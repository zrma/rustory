use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug)]
pub enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            _ => bail!("unsupported shell: {value}"),
        }
    }
}

pub fn render_hook(shell: Shell) -> String {
    match shell {
        Shell::Bash => "# rustory (rr) bash hook\n# TODO: implement".to_string(),
        Shell::Zsh => "# rustory (rr) zsh hook\n# TODO: implement".to_string(),
    }
}
