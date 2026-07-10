//! Integration tests for NVMe-oF functionality.
//!
//! These tests exercise the NVMe and NVMf APIs. Some require external
//! infrastructure (nvmf_tgt subprocess) and are marked as ignored by default.

use spdk_io::Result;
use spdk_io::nvme::TransportId;

// ============================================================================
// TransportId Unit Tests (no SPDK runtime needed)
// ============================================================================

#[test]
fn test_transport_id_pcie() {
    let trid = TransportId::pcie("0000:00:04.0");
    assert!(trid.is_ok());
    let trid = trid.unwrap();
    assert_eq!(trid.address(), "0000:00:04.0");
}

#[test]
fn test_transport_id_tcp() {
    let trid = TransportId::tcp("192.168.1.100", "4420", "nqn.2024-01.io.spdk:test");
    assert!(trid.is_ok());
    let trid = trid.unwrap();
    assert_eq!(trid.address(), "192.168.1.100");
    assert_eq!(trid.service_id(), "4420");
    assert_eq!(trid.subnqn(), "nqn.2024-01.io.spdk:test");
}

#[test]
fn test_transport_id_rdma() {
    let trid = TransportId::rdma("10.0.0.1", "4420", "nqn.2024-01.io.spdk:rdma");
    assert!(trid.is_ok());
    let trid = trid.unwrap();
    assert_eq!(trid.address(), "10.0.0.1");
    assert_eq!(trid.service_id(), "4420");
    assert_eq!(trid.subnqn(), "nqn.2024-01.io.spdk:rdma");
}

#[test]
fn test_transport_id_empty_subnqn() {
    // Empty subnqn is valid for discovery
    let trid = TransportId::tcp("127.0.0.1", "4420", "");
    assert!(trid.is_ok());
    let trid = trid.unwrap();
    assert_eq!(trid.subnqn(), "");
}

// ============================================================================
// NVMf Subprocess Test Infrastructure
// ============================================================================

/// Helper to manage an nvmf_tgt subprocess for testing.
#[cfg(test)]
mod nvmf_subprocess {
    use std::io::Write;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::Duration;

    /// Default PID file path for nvmf_tgt tracking.
    const PID_FILE: &str = "/tmp/spdk_nvmf_test.pid";

    /// NVMf target subprocess wrapper.
    pub struct NvmfTarget {
        child: Child,
        rpc_socket: String,
        pid_file: String,
    }

    impl NvmfTarget {
        /// Clean up any stale nvmf_tgt process from a previous test run.
        ///
        /// Reads the PID file and kills that specific process if it exists.
        /// Also cleans up stale socket and lock files.
        pub fn cleanup_stale(port: u16) {
            // Try to read and kill process from PID file
            if let Ok(pid_str) = std::fs::read_to_string(PID_FILE)
                && let Ok(pid) = pid_str.trim().parse::<i32>()
            {
                eprintln!("Killing stale nvmf_tgt process with PID {}", pid);
                // Send SIGKILL to the process
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                // Wait a bit for process to die
                thread::sleep(Duration::from_millis(100));
            }

            // Clean up stale files
            let _ = std::fs::remove_file(PID_FILE);
            let _ = std::fs::remove_file(format!("/tmp/spdk_nvmf_test_{}.sock", port));
            let _ = std::fs::remove_file(format!("/tmp/spdk_nvmf_test_{}.sock.lock", port));

            // Give time for resources to be released
            thread::sleep(Duration::from_millis(200));
        }

        /// Start nvmf_tgt with a null bdev and TCP listener.
        ///
        /// Returns the target and the NQN to connect to.
        pub fn start(port: u16) -> Result<(Self, String), String> {
            let nvmf_tgt_path = "/opt/spdk/bin/nvmf_tgt";
            if !Path::new(nvmf_tgt_path).exists() {
                return Err(format!("nvmf_tgt not found at {}", nvmf_tgt_path));
            }

            let rpc_socket = format!("/tmp/spdk_nvmf_test_{}.sock", port);

            // Clean up old socket
            let _ = std::fs::remove_file(&rpc_socket);

            // Choose a single core for nvmf_tgt that actually exists on this
            // machine. Prefer core 2 (leaving cores 0 and 1 for the test), but
            // fall back to the highest available core on machines with fewer
            // than 3 cores so DPDK's EAL does not reject the core mask.
            let ncores = thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            let core_index = ncores.saturating_sub(1).min(2);
            let core_mask = format!("0x{:x}", 1u64 << core_index);

            // Start nvmf_tgt
            let mut child = Command::new(nvmf_tgt_path)
                .args([
                    "-r",
                    &rpc_socket,
                    "-m",
                    core_mask.as_str(), // single core, sized to available CPUs
                    "-s",
                    "1024", // 1024 MB memory (need more for bdev pool)
                    "--no-pci",
                    "--no-huge", // Don't require hugepages
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn nvmf_tgt: {}", e))?;

            // Wait for RPC socket to be ready (or process to die)
            for _ in 0..100 {
                if Path::new(&rpc_socket).exists() {
                    break;
                }
                // Check if child died
                if let Some(status) = child.try_wait().ok().flatten() {
                    // Read stderr for error message
                    let mut stderr_output = String::new();
                    if let Some(ref mut stderr) = child.stderr {
                        use std::io::Read;
                        let _ = stderr.read_to_string(&mut stderr_output);
                    }
                    return Err(format!(
                        "nvmf_tgt exited with status {:?}: {}",
                        status,
                        stderr_output.trim()
                    ));
                }
                thread::sleep(Duration::from_millis(100));
            }

            let socket_exists = Path::new(&rpc_socket).exists();
            if !socket_exists {
                // Check if child died
                if let Some(status) = child.try_wait().ok().flatten() {
                    let mut stderr_output = String::new();
                    if let Some(ref mut stderr) = child.stderr {
                        use std::io::Read;
                        let _ = stderr.read_to_string(&mut stderr_output);
                    }
                    return Err(format!(
                        "nvmf_tgt exited before socket ready, status {:?}: {}",
                        status,
                        stderr_output.trim()
                    ));
                }
                return Err("nvmf_tgt RPC socket not ready after 10s".into());
            }

            // Check process is still running after sleep
            thread::sleep(Duration::from_millis(500));
            if !Path::new(&rpc_socket).exists() {
                // Check if child died during the sleep
                if let Some(status) = child.try_wait().ok().flatten() {
                    let mut stderr_output = String::new();
                    if let Some(ref mut stderr) = child.stderr {
                        use std::io::Read;
                        let _ = stderr.read_to_string(&mut stderr_output);
                    }
                    return Err(format!(
                        "nvmf_tgt crashed after socket creation, status {:?}: {}",
                        status,
                        stderr_output.trim()
                    ));
                }
                return Err("nvmf_tgt socket disappeared but process still running?".into());
            }

            // Write PID file for tracking
            let pid = child.id();
            if let Err(e) = std::fs::write(PID_FILE, pid.to_string()) {
                eprintln!("Warning: Failed to write PID file: {}", e);
            }

            let target = Self {
                child,
                rpc_socket: rpc_socket.clone(),
                pid_file: PID_FILE.to_string(),
            };

            // Configure via RPC
            let nqn = format!("nqn.2024-01.io.spdk:test{}", port);

            // Create malloc bdev (stores data in memory, unlike null bdev)
            target.rpc_call(r#"{"jsonrpc":"2.0","id":1,"method":"bdev_malloc_create","params":{"name":"Malloc0","num_blocks":8192,"block_size":512}}"#)?;

            // Create TCP transport
            target.rpc_call(
                r#"{"jsonrpc":"2.0","id":2,"method":"nvmf_create_transport","params":{"trtype":"tcp"}}"#
            )?;

            // Create subsystem
            target.rpc_call(&format!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"nvmf_create_subsystem","params":{{"nqn":"{}","allow_any_host":true}}}}"#,
                nqn
            ))?;

            // Add namespace
            target.rpc_call(&format!(
                r#"{{"jsonrpc":"2.0","id":4,"method":"nvmf_subsystem_add_ns","params":{{"nqn":"{}","namespace":{{"bdev_name":"Malloc0"}}}}}}"#,
                nqn
            ))?;

            // Add listener
            target.rpc_call(&format!(
                r#"{{"jsonrpc":"2.0","id":5,"method":"nvmf_subsystem_add_listener","params":{{"nqn":"{}","listen_address":{{"trtype":"tcp","adrfam":"ipv4","traddr":"127.0.0.1","trsvcid":"{}"}}}}}}"#,
                nqn, port
            ))?;

            // Wait for listener to be ready
            thread::sleep(Duration::from_millis(200));

            Ok((target, nqn))
        }

        /// Send an RPC call to nvmf_tgt.
        fn rpc_call(&self, request: &str) -> Result<String, String> {
            use std::io::Read;
            use std::os::unix::net::UnixStream;

            // Retry connection a few times - socket file may exist before SPDK is listening
            let mut stream = None;
            for attempt in 0..10 {
                match UnixStream::connect(&self.rpc_socket) {
                    Ok(s) => {
                        stream = Some(s);
                        break;
                    }
                    Err(e) => {
                        if attempt == 9 {
                            return Err(format!(
                                "Failed to connect to RPC socket after 10 attempts: {}",
                                e
                            ));
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            let mut stream = stream.unwrap();

            // Set timeout for read
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| format!("Failed to set timeout: {}", e))?;

            stream
                .write_all(request.as_bytes())
                .map_err(|e| format!("Failed to send RPC: {}", e))?;

            // Read response (SPDK JSON-RPC responses are single-line)
            let mut response = vec![0u8; 8192];
            let n = stream
                .read(&mut response)
                .map_err(|e| format!("Failed to read RPC response: {}", e))?;

            let response = String::from_utf8_lossy(&response[..n]).to_string();

            if response.contains("\"error\"") {
                return Err(format!("RPC error: {}", response));
            }

            Ok(response)
        }
    }

    impl Drop for NvmfTarget {
        fn drop(&mut self) {
            // Send SIGTERM
            let _ = self.child.kill();
            let _ = self.child.wait();

            // Clean up socket and PID file
            let _ = std::fs::remove_file(&self.rpc_socket);
            let _ = std::fs::remove_file(&self.pid_file);
        }
    }
}

// ============================================================================
// NVMf Subprocess Integration Test
// ============================================================================

/// Test NVMe-oF connection to external nvmf_tgt subprocess.
///
/// This test:
/// 1. Starts nvmf_tgt as a subprocess
/// 2. Configures it via RPC (malloc bdev, TCP transport, subsystem)
/// 3. Connects using our NVMe driver API
/// 4. Uses 2 cores with separate qpairs
/// 5. Performs read/write I/O on both cores
/// 6. Verifies data integrity
#[test]
fn test_nvmf_subprocess() -> Result<()> {
    use spdk_io::nvme::NvmeController;
    use spdk_io::{Cores, DmaBuf, SpdkApp, SpdkEvent};
    use std::process::Command;
    use std::sync::Arc;

    const TEST_PORT: u16 = 4421; // Use non-standard port to avoid conflicts

    // Clean up any stale nvmf_tgt process from previous test runs
    nvmf_subprocess::NvmfTarget::cleanup_stale(TEST_PORT);

    // Start nvmf_tgt subprocess
    eprintln!("Starting nvmf_tgt subprocess...");
    let (target, nqn) =
        nvmf_subprocess::NvmfTarget::start(TEST_PORT).map_err(spdk_io::Error::InvalidArgument)?;

    eprintln!("NVMf target started, NQN: {}", nqn);

    // Run test in SPDK app context with 2 cores
    SpdkApp::builder()
        .name("nvmf_subprocess_test")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(1024)
        .reactor_mask("0x3") // Cores 0 and 1
        .run(move || {
            use futures::executor::LocalPool;
            use futures::task::LocalSpawnExt;
            use std::cell::Cell;
            use std::rc::Rc;

            eprintln!("Running on {} cores", Cores::count());
            let main_core = Cores::current();
            eprintln!("Main callback on core {}", main_core);

            // Connect on core 0, share via Arc (NvmeController is Send + Sync)
            eprintln!("Connecting to nvmf_tgt...");
            let trid = TransportId::tcp("127.0.0.1", &TEST_PORT.to_string(), &nqn)
                .expect("Failed to create TransportId");

            let ctrlr = Arc::new(
                NvmeController::connect(&trid, None).expect("Failed to connect to nvmf_tgt"),
            );

            eprintln!("Connected! {} namespaces", ctrlr.num_namespaces());

            let sector_size = {
                let ns = ctrlr.namespace(1).expect("No namespace 1");
                eprintln!(
                    "NS1: {} sectors x {} bytes",
                    ns.num_sectors(),
                    ns.sector_size()
                );
                ns.sector_size() as usize
            };

            // Find other core
            let other_core = Cores::iter()
                .find(|&c| c != main_core)
                .expect("Expected 2 cores available");
            eprintln!("Dispatching I/O task to core {}", other_core);

            let ctrlr_clone = ctrlr.clone();
            let sector_size_clone = sector_size;

            // Dispatch to core 1 using call_on_async - returns a receiver we can poll
            // === Core 1: I/O on LBAs 100-101, returns success bool ===
            let core1_receiver = SpdkEvent::call_on_async(other_core, move || {
                eprintln!("[Core {}] Starting I/O", Cores::current());

                let mut pool = LocalPool::new();
                let spawner = pool.spawner();

                pool.run_until(async {
                    // Allocate qpair on this core
                    let qpair = Rc::new(
                        ctrlr_clone
                            .alloc_io_qpair(None)
                            .expect("Failed to alloc qpair on core 1"),
                    );

                    let ns = ctrlr_clone.namespace(1).expect("No namespace 1");
                    let mut buf = DmaBuf::alloc(sector_size_clone, sector_size_clone)
                        .expect("Failed to alloc DMA buffer");

                    // Poller for TCP completions
                    let done = Rc::new(Cell::new(false));
                    let done_clone = done.clone();
                    let qpair_clone = qpair.clone();

                    let poller_handle = spawner
                        .spawn_local_with_handle(async move {
                            while !done_clone.get() {
                                qpair_clone.process_completions(0);
                                futures_lite::future::yield_now().await;
                            }
                        })
                        .expect("Failed to spawn poller");

                    // Write pattern 0xCD to LBA 100
                    eprintln!("[Core {}] Writing 0xCD to LBA 100", Cores::current());
                    buf.as_mut_slice().fill(0xCD);
                    ns.write(&qpair, &buf, 100, 1).await.expect("Write failed");

                    // Read back
                    buf.as_mut_slice().fill(0x00);
                    ns.read(&qpair, &mut buf, 100, 1)
                        .await
                        .expect("Read failed");

                    // Verify
                    let success = buf.as_slice().iter().all(|&b| b == 0xCD);
                    eprintln!(
                        "[Core {}] Verify: {}",
                        Cores::current(),
                        if success { "PASS" } else { "FAIL" }
                    );

                    done.set(true);
                    poller_handle.await;
                    drop(qpair);

                    eprintln!("[Core {}] Done", Cores::current());
                    success
                })
            })
            .expect("Failed to dispatch to core 1");

            // === Core 0: I/O on LBAs 0-1 ===
            let mut pool = LocalPool::new();
            let spawner = pool.spawner();

            let core0_success = pool.run_until(async {
                let qpair = Rc::new(
                    ctrlr
                        .alloc_io_qpair(None)
                        .expect("Failed to alloc qpair on core 0"),
                );

                let ns = ctrlr.namespace(1).expect("No namespace 1");
                let mut buf =
                    DmaBuf::alloc(sector_size, sector_size).expect("Failed to alloc DMA buffer");

                let done = Rc::new(Cell::new(false));
                let done_clone = done.clone();
                let qpair_clone = qpair.clone();

                let poller_handle = spawner
                    .spawn_local_with_handle(async move {
                        while !done_clone.get() {
                            qpair_clone.process_completions(0);
                            futures_lite::future::yield_now().await;
                        }
                    })
                    .expect("Failed to spawn poller");

                // Write pattern 0xAB to LBA 0
                eprintln!("[Core {}] Writing 0xAB to LBA 0", Cores::current());
                buf.as_mut_slice().fill(0xAB);
                ns.write(&qpair, &buf, 0, 1).await.expect("Write failed");

                // Read back
                buf.as_mut_slice().fill(0x00);
                ns.read(&qpair, &mut buf, 0, 1).await.expect("Read failed");

                // Verify
                let success = buf.as_slice().iter().all(|&b| b == 0xAB);
                eprintln!(
                    "[Core {}] Verify: {}",
                    Cores::current(),
                    if success { "PASS" } else { "FAIL" }
                );

                done.set(true);
                poller_handle.await;
                drop(qpair);

                success
            });

            // Wait for core 1 result using the completion receiver
            let core1_success = pool
                .run_until(core1_receiver)
                .expect("Core 1 completion failed");

            // Check results
            if core0_success && core1_success {
                eprintln!("=== Test Passed: Both cores succeeded ===");
            } else {
                eprintln!(
                    "=== Test FAILED: core0={}, core1={} ===",
                    core0_success, core1_success
                );
            }

            assert!(core0_success, "Core 0 I/O failed");
            assert!(core1_success, "Core 1 I/O failed");

            SpdkApp::stop();
        })?;

    // Explicitly drop target to kill nvmf_tgt
    drop(target);

    // Clean up any remaining SPDK artifacts
    let _ = Command::new("pkill").args(["-9", "nvmf_tgt"]).status();
    let _ = std::fs::remove_file(format!("/tmp/spdk_nvmf_test_{}.sock", TEST_PORT));
    let _ = std::fs::remove_file(format!("/tmp/spdk_nvmf_test_{}.sock.lock", TEST_PORT));

    Ok(())
}
