use std::{ffi::NulError, io};

use ash::vk;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("CString error: {0}")]
    CString(#[from] NulError),
    #[error("Vulkan error: {0:?}")]
    Vk(#[from] vk::Result),
}
