#!/usr/bin/env python3
"""
Dynamic Ansible inventory script for libvirt VMs.
Auto-detects VM IP addresses from DHCP leases.
"""

import json
import os
import subprocess
import sys

# Use system libvirt connection
os.environ.setdefault("LIBVIRT_DEFAULT_URI", "qemu:///system")


def get_vm_ip(vm_name: str, network: str = "default") -> str | None:
    """Get VM IP address from libvirt DHCP leases."""
    try:
        result = subprocess.run(
            ["virsh", "net-dhcp-leases", network],
            capture_output=True,
            text=True,
            check=True,
        )
        for line in result.stdout.splitlines():
            if vm_name in line:
                # Format: Expiry MAC Protocol IP Hostname Client ID
                parts = line.split()
                for part in parts:
                    if "/" in part and "." in part:  # IP with CIDR
                        return part.split("/")[0]
    except subprocess.CalledProcessError:
        pass
    return None


def get_inventory() -> dict:
    """Generate Ansible inventory."""
    vm_name = "test-vm"
    vm_ip = get_vm_ip(vm_name)

    if not vm_ip:
        print(f"Warning: Could not detect IP for {vm_name}", file=sys.stderr)
        return {"_meta": {"hostvars": {}}}

    return {
        "test_vm": {
            "hosts": [vm_name],
        },
        "_meta": {
            "hostvars": {
                vm_name: {
                    "ansible_host": vm_ip,
                    "ansible_user": "ubuntu",
                    "ansible_ssh_common_args": "-o StrictHostKeyChecking=no",
                }
            }
        },
    }


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--list":
        print(json.dumps(get_inventory(), indent=2))
    elif len(sys.argv) == 3 and sys.argv[1] == "--host":
        # Return empty dict for host-specific vars (we use _meta)
        print(json.dumps({}))
    else:
        print("Usage: inventory.py --list | --host <hostname>", file=sys.stderr)
        sys.exit(1)
