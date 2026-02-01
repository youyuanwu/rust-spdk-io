output "vm_id" {
  description = "ID of the created VM"
  value       = libvirt_domain.vm.id
}

output "vm_name" {
  description = "Name of the created VM"
  value       = libvirt_domain.vm.name
}

output "ssh_user" {
  description = "SSH user for the VM"
  value       = var.ssh_user
}

output "ssh_command" {
  description = "SSH command to connect to the VM (get IP with: virsh domifaddr <vm-name>)"
  value       = "ssh ${var.ssh_user}@<vm-ip>  # Run 'virsh domifaddr ${libvirt_domain.vm.name}' to get the IP"
}
