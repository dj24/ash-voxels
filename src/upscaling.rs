use tracing::info;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpscalingMode {
    Off,
    Dlaa,
    Dlss,
}

impl UpscalingMode {
    pub fn from_flag(raw: Option<&str>) -> Result<Self, String> {
        match raw {
            None => Ok(Self::Off),
            Some("off") => Ok(Self::Off),
            Some("dlaa") => Ok(Self::Dlaa),
            Some("dlss") => Ok(Self::Dlss),
            Some(other) => Err(format!(
                "unsupported --upscaler mode {other:?}; expected off|dlaa|dlss"
            )),
        }
    }
}

pub fn initialize(mode: UpscalingMode) {
    match mode {
        UpscalingMode::Off => {}
        UpscalingMode::Dlaa | UpscalingMode::Dlss => {
            #[cfg(feature = "nvngx")]
            {
                info!("NVNGX upscaler requested ({mode:?}); nvngx-rs path is enabled.");
            }
            #[cfg(not(feature = "nvngx"))]
            {
                info!(
                    "NVNGX upscaler requested ({mode:?}) but `nvngx` feature is disabled at compile time; running without upscaling."
                );
            }
        }
    }
}
