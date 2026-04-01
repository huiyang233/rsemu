use std::env;

pub struct CliArgs {
    pub svd_xml: Option<String>,
    pub firmware_path: Option<String>,
    pub target: Option<String>,
    pub load_addr: Option<u32>,
    pub max_steps: Option<u64>,
    pub trace_start: Option<u32>,
    pub trace_end: Option<u32>,
    pub display_gui: bool,
    pub fast_mode: bool,
    pub dump_frames: bool,
    pub cycle_scale: u32,
}

impl CliArgs {
    pub fn parse() -> Result<Self, String> {
        let mut svd_path = None;
        let mut firmware_path = None;
        let mut target = None;
        let mut load_addr = None;
        let mut max_steps = None;
        let mut trace_start = None;
        let mut trace_end = None;
        let mut display_gui = true;
        let mut fast_mode = false;
        let mut dump_frames = false;
        let mut cycle_scale = 1u32;
        let mut iter = env::args().skip(1);

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--svd" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--svd requires a path".to_string())?;
                    svd_path = Some(value);
                }
                "--firmware" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--firmware requires a path".to_string())?;
                    firmware_path = Some(value);
                }
                "--target" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--target requires a chip identifier (e.g. STM32F407)".to_string())?;
                    target = Some(value);
                }
                "--load-addr" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--load-addr requires a hexadecimal address".to_string())?;
                    load_addr = Some(parse_u32(&value)?);
                }
                "--max-steps" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--max-steps requires an integer value".to_string())?;
                    max_steps = Some(
                        value
                            .parse()
                            .map_err(|_| format!("invalid --max-steps value: {value}"))?,
                    );
                }
                "--trace-start" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--trace-start requires an address".to_string())?;
                    trace_start = Some(parse_u32(&value)?);
                }
                "--trace-end" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--trace-end requires an address".to_string())?;
                    trace_end = Some(parse_u32(&value)?);
                }
                "--help" | "-h" => {
                    return Err(help_text());
                }
                "--gui" => {
                    display_gui = true;
                }
                "--no-gui" => {
                    display_gui = false;
                }
                "--fast" => {
                    fast_mode = true;
                }
                "--dump-frames" => {
                    dump_frames = true;
                }
                "--cycle-scale" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--cycle-scale requires an integer value".to_string())?;
                    cycle_scale = value
                        .parse()
                        .map_err(|_| format!("invalid --cycle-scale value: {value}"))?;
                    if cycle_scale == 0 {
                        return Err("--cycle-scale must be >= 1".to_string());
                    }
                }
                other => {
                    return Err(format!("unknown argument: {other}\n\n{}", help_text()));
                }
            }
        }

        let svd_xml = svd_path
            .as_deref()
            .map(std::fs::read_to_string)
            .transpose()
            .map_err(|err| {
                format!(
                    "failed to read SVD file {}: {err}",
                    svd_path.as_deref().unwrap_or_default()
                )
            })?;

        Ok(Self {
            svd_xml,
            firmware_path,
            target,
            load_addr,
            max_steps,
            trace_start,
            trace_end,
            display_gui,
            fast_mode,
            dump_frames,
            cycle_scale,
        })
    }
}

fn help_text() -> String {
    "usage: rsemu-cli --svd path/to/device.svd [--firmware path/to/firmware.{bin,elf}] [--target STM32F103|STM32F407] [--load-addr ADDR] [--max-steps N] [--trace-start ADDR] [--trace-end ADDR] [--gui|--no-gui] [--fast] [--dump-frames] [--cycle-scale N]".to_string()
}

fn parse_u32(value: &str) -> Result<u32, String> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).map_err(|_| format!("invalid address: {value}"))
    } else {
        value
            .parse()
            .map_err(|_| format!("invalid address: {value}"))
    }
}
