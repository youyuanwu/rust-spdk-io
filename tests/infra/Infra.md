# Test Infrastructure

Terraform configuration for provisioning QEMU/KVM virtual machines using the libvirt provider.

## Features

- **Ubuntu 24.04 LTS** (Noble) cloud image
- **NVMe emulation** via QEMU command-line passthrough (appears as `/dev/nvme0n1`)
- **Cloud-init** for automatic SSH key configuration
- **KVM acceleration** with fallback to software emulation (`--no-kvm`)
- **CMake integration** for build system targets

## Prerequisites

1. **Install libvirt and QEMU/KVM:**
   ```bash
   sudo apt install qemu-kvm libvirt-daemon-system libvirt-dev virtinst qemu-utils
   ```

2. **Add your user to the libvirt group:**
   ```bash
   sudo usermod -aG libvirt $USER
   newgrp libvirt
   ```

3. **Verify KVM is working:**
   ```bash
   virsh list --all
   ```

4. **Create the default storage pool** (if it doesn't exist):
   ```bash
   sudo mkdir -p /var/lib/libvirt/images
   sudo virsh pool-define-as default dir --target /var/lib/libvirt/images
   sudo virsh pool-start default
   sudo virsh pool-autostart default
   ```

5. **Install Terraform** (if not installed):
   ```bash
   cd /tmp
   wget https://releases.hashicorp.com/terraform/1.14.4/terraform_1.14.4_linux_amd64.zip
   unzip terraform_1.14.4_linux_amd64.zip
   sudo mv terraform /usr/local/bin/
   terraform --version
   ```

   Or use apt:
   ```bash
   wget -O - https://apt.releases.hashicorp.com/gpg | sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg
   echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(grep -oP '(?<=UBUNTU_CODENAME=).*' /etc/os-release || lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/hashicorp.list
   sudo apt update && sudo apt install terraform
   ```

6. **SSH key** - Ensure you have an SSH key at `~/.ssh/id_rsa.pub` (or configure a custom key in `terraform.tfvars`)

## Quick Start

### Using helper scripts (recommended)

```bash
# Start VM with KVM acceleration
./start-vm.sh

# Start VM with auto-approve (no prompts)
./start-vm.sh -y

# Start VM without KVM (software emulation, slower)
./start-vm.sh --no-kvm

# Destroy VM
./destroy-vm.sh -y
```

### Using CMake targets

```bash
cd build
cmake ..
make vm-start          # Start VM with KVM
make vm-start-no-kvm   # Start VM without KVM
make vm-destroy        # Destroy VM
make e2e-test          # Run E2E tests
```

### Manual Terraform commands

```bash
terraform init
terraform plan
terraform apply
terraform destroy
```

## Connecting to the VM

```bash
# Get SSH command from Terraform output
terraform output ssh_command

# Or connect directly (after VM starts)
ssh -o StrictHostKeyChecking=no ubuntu@<VM_IP>

# Get VM IP
virsh net-dhcp-leases default
```

## NVMe Emulation

The VM includes an emulated NVMe disk (1GB by default) that appears as `/dev/nvme0n1`.

**Configuration** (in `terraform.tfvars`):
```hcl
nvme_enabled   = true          # Enable/disable NVMe
nvme_disk_size = 1073741824    # Size in bytes (1GB)
```

**How it works:**
- NVMe disk is created at `/tmp/<vm_name>-nvme.qcow2` using `qemu-img`
- Attached via QEMU command-line passthrough (`qemu:commandline`)
- Placed at PCI address `0x10` to avoid conflicts with other devices

**Verify in VM:**
```bash
lsblk | grep nvme
ls -la /dev/nvme*
```

## Files

| File | Description |
|------|-------------|
| `main.tf` | Main Terraform configuration with provider and resources |
| `variables.tf` | Variable definitions |
| `outputs.tf` | Output definitions |
| `terraform.tfvars` | Variable values (VM name, memory, NVMe settings) |
| `start-vm.sh` | Helper script to provision the VM |
| `destroy-vm.sh` | Helper script to destroy the VM |

## Script Options

| Option | Description |
|--------|-------------|
| `-y`, `--yes` | Auto-approve without prompting |
| `--no-kvm` | Use software emulation instead of KVM |
| `--no-color` | Disable colored output (for CI) |
| `-h`, `--help` | Show help |

## Customization

Modify `terraform.tfvars` to customize:

```hcl
vm_name        = "test-vm"
memory_mb      = 2048           # RAM in MB
vcpu_count     = 2              # Virtual CPUs
disk_size      = 10737418240    # OS disk size (10GB)
nvme_enabled   = true           # Enable NVMe disk
nvme_disk_size = 1073741824     # NVMe disk size (1GB)
```

## Troubleshooting

### Console access
```bash
virsh console test-vm
# Press Ctrl+] to exit
```

### Check VM status
```bash
virsh list --all
virsh dominfo test-vm
```

### View DHCP leases
```bash
virsh net-dhcp-leases default
```

### Check cloud-init logs (in VM)
```bash
sudo cat /var/log/cloud-init-output.log
```
