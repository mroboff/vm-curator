use super::*;

#[test]
fn test_display_manager_service_names() {
    assert_eq!(DisplayManager::Gdm.service_name(), "gdm");
    assert_eq!(DisplayManager::Sddm.service_name(), "sddm");
    assert_eq!(DisplayManager::Lightdm.service_name(), "lightdm");
}

#[test]
fn test_gpu_driver_modules() {
    assert_eq!(GpuDriver::Nvidia.module_name(), "nvidia");
    assert!(!GpuDriver::Nvidia.dependent_modules().is_empty());
}
