variable "libvirt_uri" {
  description = "Libvirt connection URI"
  type        = string
  default     = "qemu:///system"
}

variable "vm_name" {
  description = "Name of the virtual machine"
  type        = string
  default     = "test-vm"
}

variable "memory_mb" {
  description = "Memory allocation in MB"
  type        = number
  default     = 2048
}

variable "vcpu_count" {
  description = "Number of virtual CPUs"
  type        = number
  default     = 2
}

variable "disk_size" {
  description = "Disk size in bytes (default 10GB)"
  type        = number
  default     = 10737418240 # 10 GB
}

variable "storage_pool" {
  description = "Libvirt storage pool name"
  type        = string
  default     = "default"
}

variable "network_name" {
  description = "Libvirt network name"
  type        = string
  default     = "default"
}

variable "base_image_url" {
  description = "URL or path to the base OS image"
  type        = string
  default     = "https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img"
}

variable "ssh_user" {
  description = "SSH username for the VM"
  type        = string
  default     = "ubuntu"
}

variable "ssh_public_key" {
  description = "SSH public key for authentication (defaults to ~/.ssh/id_rsa.pub)"
  type        = string
  default     = ""
}

variable "ssh_key_path" {
  description = "Path to SSH public key file"
  type        = string
  default     = "~/.ssh/id_rsa.pub"
}

variable "use_kvm" {
  description = "Use KVM hardware acceleration (set to false for software emulation)"
  type        = bool
  default     = true
}
