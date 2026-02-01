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
            echo "  --no-kvm  Use QEMU software emulation config (must match how VM was created)"
            echo "  -y, --yes Auto-approve without prompting"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

TF_VAR_ARGS=""
if [[ "$USE_KVM" == "false" ]]; then
    TF_VAR_ARGS="-var=use_kvm=false"
fi

echo "=== QEMU/KVM VM Destruction ==="

# Check if terraform state exists
if [[ ! -f "terraform.tfstate" ]]; then
    echo "No Terraform state found. Nothing to destroy."
    exit 0
fi

# Show current resources
echo "Current resources:"
terraform state list 2>/dev/null || echo "(none)"
echo ""

# Confirm destruction
if [[ "$AUTO_APPROVE" == "true" ]]; then
    REPLY=y
else
    read -p "Are you sure you want to destroy the VM? (y/N) " -n 1 -r
    echo ""
fi

if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Destroying VM..."
    terraform destroy $TF_VAR_ARGS -auto-approve

    echo ""
    echo "=== VM Destroyed Successfully ==="
else
    echo "Aborted."
    exit 0
fi
