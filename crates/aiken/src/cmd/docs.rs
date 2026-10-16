use aiken_project::{
    docs,
    watch::{self, watch_project, with_project},
};
use std::{path::PathBuf, process};

#[derive(clap::Args)]
#[clap(disable_version_flag(true))]
/// Build the documentation for an Aiken project
pub struct Args {
    /// Path to project
    directory: Option<PathBuf>,

    /// Deny warnings; warnings will be treated as errors
    #[clap(short = 'D', long)]
    deny: bool,

    /// Silence warnings; warnings will not be printed
    #[clap(short = 'S', long)]
    silent: bool,

    /// When enabled, re-run the command on file changes instead of exiting
    #[clap(short, long)]
    watch: bool,

    /// When enabled, also generate documentation from dependencies.
    #[clap(long)]
    include_dependencies: bool,

    /// Output directory for the documentation
    #[clap(short = 'o', long)]
    destination: Option<PathBuf>,

    /// Output format: a static website, or MDX pages for a Nimbus documentation site
    #[clap(long, value_enum, default_value_t = Format::Html)]
    format: Format,

    /// With `--format markdown`, the URL path the pages are mounted at on the site (e.g.
    /// `/stdlib`), used for links between them
    #[clap(long, default_value = "/")]
    base: String,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Html,
    Markdown,
}

pub fn exec(
    Args {
        directory,
        deny,
        silent,
        watch,
        destination,
        include_dependencies,
        format,
        base,
    }: Args,
) -> miette::Result<()> {
    let format = match format {
        Format::Html => docs::Format::Html,
        Format::Markdown => docs::Format::Markdown { base },
    };

    let result = if watch {
        watch_project(directory.as_deref(), watch::default_filter, 500, |p| {
            p.docs(destination.clone(), include_dependencies, format.clone())
        })
    } else {
        with_project(directory.as_deref(), deny, silent, true, |p| {
            p.docs(destination.clone(), include_dependencies, format.clone())
        })
    };

    result.map_err(|_| process::exit(1))
}
