# Test Infrastructure

Terraform configuration for provisioning QEMU/KVM virtual machines using the libvirt provider.

## Prerequisites

1. **Install libvirt and QEMU/KVM:**
   ```bash
   sudo apt install qemu-kvm libvirt-daemon-system libvirt-dev virtinst
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

## Usage

1. **Configure variables:**
   ```bash
   cp terraform.tfvars.example terraform.tfvars
   # Edit terraform.tfvars with your SSH public key and other settings
   ```

2. **Initialize Terraform:**
   ```bash
   terraform init
   ```

3. **Plan the deployment:**
   ```bash
   terraform plan
   ```

4. **Apply the configuration:**
   ```bash
   terraform apply
   ```

5. **Connect to the VM:**
   ```bash
   terraform output ssh_command
   # Then run the outputted SSH command
   ```

6. **Destroy the VM:**
   ```bash
   terraform destroy
   ```

## Files

| File | Description |
|------|-------------|
| `main.tf` | Main Terraform configuration with provider and resources |
| `variables.tf` | Variable definitions |
| `outputs.tf` | Output definitions |
| `terraform.tfvars.example` | Example variable values |

## Customization

- Modify `variables.tf` defaults or create `terraform.tfvars` to customize:
  - VM name, memory, and CPU count
  - Disk size
  - Base OS image (defaults to Ubuntu 22.04 cloud image)
  - Network configuration

```sh
# console
virsh console test-vm

virsh net-dhcp-leases default
```
