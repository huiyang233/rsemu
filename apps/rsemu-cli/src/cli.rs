use std::env;

pub struct CliArgs {
    pub board_path: String,
    pub max_steps: Option<u64>,
    pub no_gui: bool,
    pub fast_mode: bool,
    pub dump_frames: bool,
}

impl CliArgs {
    pub fn parse() -> Result<Self, String> {
        let mut board_path = "board.toml".to_string();
        let mut max_steps = None;
        let mut no_gui = false;
        let mut fast_mode = false;
        let mut dump_frames = false;
        let mut iter = env::args().skip(1);

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--board" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--board requires a path".to_string())?;
                    board_path = value;
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
                "--help" | "-h" => {
                    return Err(help_text());
                }
                "--no-gui" => {
                    no_gui = true;
                }
                "--fast" => {
                    fast_mode = true;
                }
                "--dump-frames" => {
                    dump_frames = true;
                }
                other => {
                    return Err(format!("unknown argument: {other}\n\n{}", help_text()));
                }
            }
        }

        Ok(Self {
            board_path,
            max_steps,
            no_gui,
            fast_mode,
            dump_frames,
        })
    }
}

fn help_text() -> String {
    "usage: rsemu-cli [--board board.toml] [--max-steps N] [--no-gui] [--fast] [--dump-frames]"
        .to_string()
}
