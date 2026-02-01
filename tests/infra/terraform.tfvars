# Terraform variables for QEMU/KVM VM

libvirt_uri    = "qemu:///system"
vm_name        = "test-vm"
memory_mb      = 2048
vcpu_count     = 2
disk_size      = 10737418240 # 10 GB
storage_pool   = "default"
network_name   = "default"
base_image_url = "https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img"
ssh_user       = "ubuntu"

# SSH key is loaded automatically from ~/.ssh/id_rsa.pub via variables.tf default
# Uncomment below to override with a specific key:
# ssh_public_key = "ssh-rsa AAAA... your-key-here"
