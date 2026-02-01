terraform {
  required_version = ">= 1.0"

  required_providers {
    libvirt = {
      source  = "dmacvicar/libvirt"
      version = "~> 0.9"
    }
  }
}

# Configure the Libvirt provider
provider "libvirt" {
  uri = var.libvirt_uri
}

# Load SSH key from file if not provided directly
locals {
  ssh_public_key = var.ssh_public_key != "" ? var.ssh_public_key : file(pathexpand(var.ssh_key_path))
}

# Base OS image volume (downloaded from URL)
resource "libvirt_volume" "base_image" {
  name = "${var.vm_name}-base.qcow2"
  pool = var.storage_pool

  create = {
    content = {
      url = var.base_image_url
    }
  }
}

# VM disk volume (overlay on base image)
resource "libvirt_volume" "vm_disk" {
  name     = "${var.vm_name}-disk.qcow2"
  pool     = var.storage_pool
  capacity = var.disk_size

  target = {
    format = {
      type = "qcow2"
    }
  }

  backing_store = {
    path = libvirt_volume.base_image.path
    format = {
      type = "qcow2"
    }
  }
}

# Cloud-init configuration
resource "libvirt_cloudinit_disk" "cloudinit" {
  name = "${var.vm_name}-cloudinit"

  meta_data = yamlencode({
    instance-id    = "${var.vm_name}-${formatdate("YYYYMMDDhhmmss", timestamp())}"
    local-hostname = var.vm_name
  })

  user_data = <<-EOF
    #cloud-config
    hostname: ${var.vm_name}
    users:
      - name: ${var.ssh_user}
        sudo: ALL=(ALL) NOPASSWD:ALL
        shell: /bin/bash
        ssh_authorized_keys:
          - ${local.ssh_public_key}
    package_update: false
  EOF

  network_config = yamlencode({
    version = 2
    ethernets = {
      id0 = {
        match = {
          name = "en*"
        }
        dhcp4 = true
      }
    }
  })
}

# Cloud-init volume
resource "libvirt_volume" "cloudinit" {
  name = "${var.vm_name}-cloudinit.iso"
  pool = var.storage_pool

  create = {
    content = {
      url = libvirt_cloudinit_disk.cloudinit.path
    }
  }
}

# Virtual machine domain
resource "libvirt_domain" "vm" {
  name        = var.vm_name
  memory      = var.memory_mb
  memory_unit = "MiB"
  vcpu        = var.vcpu_count
  type        = var.use_kvm ? "kvm" : "qemu"
  running     = true
  autostart   = true

  os = {
    type      = "hvm"
    type_arch = "x86_64"
    boot_devices = [
      { dev = "hd" }
    ]
  }

  cpu = {
    mode = var.use_kvm ? "host-passthrough" : "custom"
    model = var.use_kvm ? null : "qemu64"
  }

  devices = {
    disks = [
      {
        driver = {
          type = "qcow2"
        }
        source = {
          volume = {
            pool   = var.storage_pool
            volume = libvirt_volume.vm_disk.name
          }
        }
        target = {
          dev = "vda"
          bus = "virtio"
        }
      },
      {
        device = "cdrom"
        source = {
          volume = {
            pool   = var.storage_pool
            volume = libvirt_volume.cloudinit.name
          }
        }
        target = {
          dev = "sda"
          bus = "sata"
        }
        readonly = true
      }
    ]

    interfaces = [
      {
        model = {
          type = "virtio"
        }
        source = {
          network = {
            network = var.network_name
          }
        }
      }
    ]

    consoles = [
      {
        target = {
          type = "serial"
          port = 0
        }
      }
    ]

    graphics = [
      {
        vnc = {
          auto_port = true
        }
      }
    ]

    channels = [
      {
        target = {
          virt_io = {
            name = "org.qemu.guest_agent.0"
          }
        }
      }
    ]
  }
}
