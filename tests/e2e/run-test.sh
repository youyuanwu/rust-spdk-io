#!/bin/bash
# Run the e2e echo test against the test VM

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Use system libvirt connection
export LIBVIRT_DEFAULT_URI="qemu:///system"

# Check if ansible is installed
if ! command -v ansible-playbook &> /dev/null; then
    echo "Error: ansible-playbook not found. Install with: sudo apt install ansible"
    exit 1
fi

# Check if VM is running
if ! virsh list --name 2>/dev/null | grep -q "^test-vm$"; then
    echo "Error: test-vm is not running. Start it with: cd ../infra && ./start-vm.sh"
    exit 1
fi

# Check if VM has an IP (DHCP lease)
if ! virsh net-dhcp-leases default 2>/dev/null | grep -q "test-vm"; then
    echo "Error: test-vm has no DHCP lease. Wait for VM to boot or check network config."
    exit 1
fi

echo "Running e2e echo test..."
ansible-playbook -i "$SCRIPT_DIR/inventory.py" "$SCRIPT_DIR/test_echo.yml"

echo ""
echo "✓ Test passed!"
