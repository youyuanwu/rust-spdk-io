#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Use system libvirt connection (same as terraform libvirt provider default)
export LIBVIRT_DEFAULT_URI="qemu:///system"

# Parse arguments
USE_KVM=true
AUTO_APPROVE=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --no-kvm)
            USE_KVM=false
            shift
            ;;
        -y|--yes)
            AUTO_APPROVE=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--no-kvm] [-y]"
            echo "  --no-kvm  Use QEMU software emulation instead of KVM (slower)"
            echo "  -y, --yes Auto-approve without prompting"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ "$USE_KVM" == "true" ]]; then
    echo "=== QEMU/KVM VM Provisioning ==="
else
    echo "=== QEMU VM Provisioning (no KVM, software emulation) ==="
    echo "Warning: Software emulation is much slower than KVM"
fi

TF_VAR_ARGS=""
if [[ "$USE_KVM" == "false" ]]; then
    TF_VAR_ARGS="-var=use_kvm=false"
fi

# Check if libvirt is running
if ! systemctl is-active --quiet libvirtd; then
    echo "Error: libvirtd is not running. Start it with: sudo systemctl start libvirtd"
    exit 1
fi

# Check if user can access libvirt
if ! virsh list &>/dev/null; then
    echo "Error: Cannot connect to libvirt. Make sure your user is in the 'libvirt' group."
    echo "Run: sudo usermod -aG libvirt $USER && newgrp libvirt"
    exit 1
fi

# Check SSH key exists
SSH_KEY_PATH="${SSH_KEY_PATH:-$HOME/.ssh/id_rsa.pub}"
if [[ ! -f "$SSH_KEY_PATH" ]]; then
    echo "Error: SSH public key not found at $SSH_KEY_PATH"
    echo "Generate one with: ssh-keygen -t ed25519"
    exit 1
fi

# Check if default storage pool exists
if ! virsh pool-info default &>/dev/null; then
    echo "Error: Storage pool 'default' not found."
    echo "Create it with:"
    echo "  sudo mkdir -p /var/lib/libvirt/images"
    echo "  sudo virsh pool-define-as default dir --target /var/lib/libvirt/images"
    echo "  sudo virsh pool-start default"
    echo "  sudo virsh pool-autostart default"
    exit 1
fi

# Ensure default network is running
if ! virsh net-info default &>/dev/null; then
    echo "Starting default network..."
    virsh net-start default || true
fi

# Initialize Terraform if needed
if [[ ! -d ".terraform" ]]; then
    echo "Initializing Terraform..."
    terraform init
fi

# Plan and apply
echo "Planning infrastructure..."
terraform plan $TF_VAR_ARGS -out=tfplan

echo ""
if [[ "$AUTO_APPROVE" == "true" ]]; then
    REPLY=y
else
    read -p "Apply this plan? (y/N) " -n 1 -r
    echo ""
fi

if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Applying configuration..."
    terraform apply $TF_VAR_ARGS tfplan
    rm -f tfplan

    echo ""
    echo "=== VM Created Successfully ==="
    echo ""

    # Ensure VM is running
    VM_NAME=$(terraform output -raw vm_name 2>/dev/null || echo "test-vm")
    if ! virsh list --name | grep -q "^${VM_NAME}$"; then
        echo "Starting VM..."
        virsh start "$VM_NAME" || true
    fi

    # Wait for VM to get DHCP lease
    echo "Waiting for VM to boot and get IP address..."
    for i in {1..30}; do
        if virsh net-dhcp-leases default 2>/dev/null | grep -q "$VM_NAME"; then
            echo "VM is ready!"
            break
        fi
        echo "  Waiting... ($i/30)"
        sleep 5
    done

    echo ""
    terraform output
    echo ""
    echo "Connect with:"
    VM_IP=$(virsh net-dhcp-leases default 2>/dev/null | grep "$VM_NAME" | awk '{print $5}' | cut -d'/' -f1)
    if [[ -n "$VM_IP" ]]; then
        echo "  ssh ubuntu@$VM_IP"
    else
        echo "  ssh ubuntu@<vm-ip>  # Run 'virsh net-dhcp-leases default' to get the IP"
    fi
else
    echo "Aborted."
    rm -f tfplan
    exit 0
fi
