//! Compare AI graph semantics despite node renumbering during serialization.
use anyhow_ext::{Context, Result};
use roead::{aamp::ParameterIO, sarc::Sarc};
use uk_content::actor::params::aiprog::AIProgram;

fn read_actor(path: &std::ffi::OsStr, member: &str) -> Result<AIProgram> {
    let bytes = std::fs::read(path)?;
    let decoded = roead::yaz0::decompress(&bytes)?;
    let sarc = Sarc::new(&decoded)?;
    let pio = ParameterIO::from_binary(sarc.get_data(member).context("No AI program in actor pack")?)?;
    Ok(AIProgram::try_from(&pio)?)
}

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    anyhow_ext::ensure!(args.len() == 3 || args.len() == 4,
        "Usage: check_exported_ai_program <source.sbactorpack> <export.sbactorpack> <member.baiprog> [raw-diagnostic]");
    let member = args[2].to_str().context("Invalid member name")?;
    if let Some(destination) = args.get(3) {
        let bytes = std::fs::read(&args[1])?;
        let decoded = roead::yaz0::decompress(&bytes)?;
        std::fs::write(std::path::PathBuf::from(destination).with_extension("actor"), &decoded)?;
        let sarc = Sarc::new(&decoded)?;
        std::fs::write(destination, sarc.get_data(member).context("No AI program")?)?;
    }
    let before = read_actor(&args[0], member)?;
    let after = read_actor(&args[1], member)?;
    anyhow_ext::ensure!(before == after, "Export changed the AI graph or custom parameters");
    println!("AI graph and custom parameters match the original actor pack");
    Ok(())
}
