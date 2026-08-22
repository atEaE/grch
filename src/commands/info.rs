use crate::dir;

pub fn run() -> anyhow::Result<()> {
    let version = env!("CARGO_PKG_VERSION");

    println!("grch v{}", version);
    println!();

    let cache_dir = dir::romdat_cache_dir()?;
    let custom_dat_dir = dir::custom_dat_dir()?;

    println!("cache dir:    {}", cache_dir.display());
    println!("custom dir:   {}", custom_dat_dir.display());
    println!("dat official: https://github.com/libretro/libretro-database");
    Ok(())
}
