use anyhow::Result;
use bose_dfu::{ensure_idle, enter_dfu, leave_dfu, upload};
use structopt::StructOpt;
use thiserror::Error;

const BOSE_VID: u16 = 0x05a7;

const SUPPORTED_NONDFU_PIDS: &[u16] = &[
    0x40fe, // Bose Color II SoundLink
];

const SUPPORTED_DFU_PIDS: &[u16] = &[
    0x400d, // Bose Color II SoundLink
];

fn get_mode(pid: u16) -> Option<DeviceMode> {
    match pid {
        v if SUPPORTED_NONDFU_PIDS.contains(&v) => Some(DeviceMode::Normal),
        v if SUPPORTED_DFU_PIDS.contains(&v) => Some(DeviceMode::Dfu),
        _ => None,
    }
}

#[derive(StructOpt, Debug)]
#[structopt(name = "bose-dfu")]
enum Opt {
    /// List all connected Bose HID devices (vendor ID 0x05a7)
    List,

    /// Put a device into DFU mode
    EnterDfu {
        #[structopt(flatten)]
        spec: DeviceSpec,
    },

    /// Take a device out of DFU mode
    LeaveDfu {
        #[structopt(flatten)]
        spec: DeviceSpec,
    },

    /// Read the firmware of a device in DFU mode
    Upload {
        #[structopt(flatten)]
        spec: DeviceSpec,

        #[structopt(parse(from_os_str))]
        file: std::path::PathBuf,
    },
}

#[derive(Error, Debug)]
enum MatchError {
    #[error("no devices match specification")]
    NoDevices,

    #[error("multiple devices match specification")]
    MultipleDevices,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum DeviceMode {
    Normal,
    Dfu,
}

#[derive(StructOpt, Debug)]
struct DeviceSpec {
    /// Serial number
    #[structopt(short)]
    serial: Option<String>,

    /// Product ID (vendor ID is always matched against Bose's, 0x05a7)
    #[structopt(short)]
    pid: Option<u16>,

    /// DFU/normal mode (determined using product ID for known devices)
    #[structopt(skip)]
    required_mode: Option<DeviceMode>,
}

impl DeviceSpec {
    fn matches(&self, device: &hidapi::DeviceInfo) -> bool {
        if device.vendor_id() != BOSE_VID {
            return false;
        }

        if let Some(ref x) = self.serial {
            if device.serial_number() != Some(x) {
                return false;
            }
        }

        if let Some(x) = self.pid {
            if device.product_id() != x {
                return false;
            }
        }

        if let Some(mode) = self.required_mode {
            // TODO: Handle unknown devices
            if get_mode(device.product_id()) != Some(mode) {
                return false;
            }
        }

        true
    }

    fn get_device(&self, hidapi: &hidapi::HidApi) -> Result<hidapi::HidDevice> {
        let mut candidates = hidapi.device_list().filter(|d| self.matches(d));

        match candidates.next() {
            None => Err(MatchError::NoDevices.into()),
            Some(dev) => {
                if candidates.next().is_some() {
                    Err(MatchError::MultipleDevices.into())
                } else {
                    dev.open_device(hidapi).map_err(Into::into)
                }
            }
        }
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::new()
            .filter_or("BOSE_DFU_LOG", "info")
            .write_style("BOSE_DFU_LOG_STYLE"),
    )
    .init();

    let mode = Opt::from_args();

    let api = hidapi::HidApi::new()?;

    match mode {
        Opt::List => list(&api),
        Opt::EnterDfu { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Normal),
                ..spec
            };
            enter_dfu(&spec.get_device(&api)?)?;
        }
        Opt::LeaveDfu { spec } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };
            leave_dfu(&spec.get_device(&api)?)?;
        }
        Opt::Upload { spec, file: path } => {
            let spec = DeviceSpec {
                required_mode: Some(DeviceMode::Dfu),
                ..spec
            };

            let device = &spec.get_device(&api)?;
            ensure_idle(device)?;

            let mut file = std::fs::File::create(path)?;
            upload(device, &mut file)?;
        }
    };

    Ok(())
}

fn list(hidapi: &hidapi::HidApi) {
    let all_spec = DeviceSpec {
        serial: None,
        pid: None,
        required_mode: None,
    };
    for dev in hidapi.device_list().filter(|d| all_spec.matches(d)) {
        let support_status = match get_mode(dev.product_id()) {
            Some(DeviceMode::Normal) => "not in DFU mode, known device",
            Some(DeviceMode::Dfu) => "in DFU mode, known device",
            None => "unknown device, proceed at your own risk",
        };

        println!(
            "{} {} [{}]",
            dev.serial_number().unwrap_or("INVALID"),
            dev.product_string().unwrap_or("INVALID"),
            support_status,
        );
    }
}
