use std::{ffi::OsString, mem, str::FromStr};

use anyhow::{bail, format_err, Error, Result};
use camino::Utf8PathBuf;
use lexopt::{
    Arg::{Long, Short, Value},
    ValueExt,
};

use crate::{
    env,
    process::ProcessBuilder,
    term::{self, Coloring},
};

// TODO: add --config option and passthrough to cargo-config: https://github.com/rust-lang/cargo/pull/10755/

#[derive(Debug)]
// #[clap(
//     bin_name = "cargo llvm-cov",
//     about(ABOUT),
//     version,
//     max_term_width(MAX_TERM_WIDTH),
//     setting(AppSettings::DeriveDisplayOrder)
// )]
pub(crate) struct Args {
    // #[clap(subcommand)]
    pub(crate) subcommand: Subcommand,

    // #[clap(flatten)]
    cov: LlvmCovOptions,

    // https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/instrument-coverage.html#including-doc-tests
    /// Including doc tests (unstable)
    ///
    /// This flag is unstable.
    /// See <https://github.com/taiki-e/cargo-llvm-cov/issues/2> for more.
    // #[clap(long)]
    pub(crate) doctests: bool,

    // =========================================================================
    // `cargo test` options
    // https://doc.rust-lang.org/nightly/cargo/commands/cargo-test.html
    /// Generate coverage report without running tests
    // #[clap(long, conflicts_with = "no-report")]
    pub(crate) no_run: bool,
    /// Run all tests regardless of failure
    // #[clap(long)]
    pub(crate) no_fail_fast: bool,
    /// Run all tests regardless of failure and generate report
    ///
    /// If tests failed but report generation succeeded, exit with a status of 0.
    // #[clap(
    //     long,
    //     // --ignore-run-fail implicitly enable --no-fail-fast.
    //     conflicts_with = "no-fail-fast",
    // )]
    pub(crate) ignore_run_fail: bool,
    // /// Display one character per test instead of one line
    // #[clap(short, long, conflicts_with = "verbose")]
    // pub(crate) quiet: bool,
    /// Test only this package's library unit tests
    // #[clap(long, conflicts_with = "doc", conflicts_with = "doctests")]
    pub(crate) lib: bool,
    /// Test only the specified binary
    // #[clap(
    //     long,
    //     multiple_occurrences = true,
    //     value_name = "NAME",
    //     conflicts_with = "doc",
    //     conflicts_with = "doctests"
    // )]
    pub(crate) bin: Vec<String>,
    /// Test all binaries
    // #[clap(long, conflicts_with = "doc", conflicts_with = "doctests")]
    pub(crate) bins: bool,
    /// Test only the specified example
    // #[clap(
    //     long,
    //     multiple_occurrences = true,
    //     value_name = "NAME",
    //     conflicts_with = "doc",
    //     conflicts_with = "doctests"
    // )]
    pub(crate) example: Vec<String>,
    /// Test all examples
    // #[clap(long, conflicts_with = "doc", conflicts_with = "doctests")]
    pub(crate) examples: bool,
    /// Test only the specified test target
    // #[clap(
    //     long,
    //     multiple_occurrences = true,
    //     value_name = "NAME",
    //     conflicts_with = "doc",
    //     conflicts_with = "doctests"
    // )]
    pub(crate) test: Vec<String>,
    /// Test all tests
    // #[clap(long, conflicts_with = "doc", conflicts_with = "doctests")]
    pub(crate) tests: bool,
    /// Test only the specified bench target
    // #[clap(
    //     long,
    //     multiple_occurrences = true,
    //     value_name = "NAME",
    //     conflicts_with = "doc",
    //     conflicts_with = "doctests"
    // )]
    pub(crate) bench: Vec<String>,
    /// Test all benches
    // #[clap(long, conflicts_with = "doc", conflicts_with = "doctests")]
    pub(crate) benches: bool,
    /// Test all targets
    // #[clap(long, conflicts_with = "doc", conflicts_with = "doctests")]
    pub(crate) all_targets: bool,
    /// Test only this library's documentation (unstable)
    ///
    /// This flag is unstable because it automatically enables --doctests flag.
    /// See <https://github.com/taiki-e/cargo-llvm-cov/issues/2> for more.
    // #[clap(long)]
    pub(crate) doc: bool,
    /// Package to run tests for
    // cargo allows the combination of --package and --workspace, but we reject
    // it because the situation where both flags are specified is odd.
    // #[clap(
    //     short,
    //     long,
    //     multiple_occurrences = true,
    //     value_name = "SPEC",
    //     conflicts_with = "workspace"
    // )]
    pub(crate) package: Vec<String>,
    /// Test all packages in the workspace
    // #[clap(long, visible_alias = "all")]
    pub(crate) workspace: bool,
    /// Exclude packages from both the test and report
    // #[clap(long, multiple_occurrences = true, value_name = "SPEC", requires = "workspace")]
    pub(crate) exclude: Vec<String>,
    /// Exclude packages from the test (but not from the report)
    // #[clap(long, multiple_occurrences = true, value_name = "SPEC", requires = "workspace")]
    pub(crate) exclude_from_test: Vec<String>,
    /// Exclude packages from the report (but not from the test)
    // #[clap(long, multiple_occurrences = true, value_name = "SPEC")]
    pub(crate) exclude_from_report: Vec<String>,

    // #[clap(flatten)]
    pub(crate) build: BuildOptions,

    // #[clap(flatten)]
    pub(crate) manifest: ManifestOptions,

    // /// Unstable (nightly-only) flags to Cargo
    // #[clap(short = 'Z', multiple_occurrences = true, value_name = "FLAG")]
    // pub(crate) unstable_flags: Vec<String>,
    // /// Arguments for the test binary
    // #[clap(last = true)]
    // pub(crate) args: Vec<String>,
    pub(crate) cargo_args: Vec<String>,
    pub(crate) rest: Vec<String>,
}

impl Args {
    pub(crate) fn parse() -> Result<Self> {
        const SUBCMD: &str = "llvm-cov";

        // rustc/cargo args must be valid Unicode
        // https://github.com/rust-lang/rust/blob/1.62.0/compiler/rustc_driver/src/lib.rs#L1325-L1335
        fn handle_args(
            args: impl IntoIterator<Item = impl Into<OsString>>,
        ) -> impl Iterator<Item = Result<String>> {
            args.into_iter().enumerate().map(|(i, arg)| {
                arg.into().into_string().map_err(|arg| {
                    format_err!("argument {} is not valid Unicode: {:?}", i + 1, arg)
                })
            })
        }

        let mut raw_args = handle_args(env::args_os());
        raw_args.next(); // cargo
        match raw_args.next().transpose()? {
            Some(a) if a == SUBCMD => {}
            Some(a) => bail!("expected subcommand '{}', found argument '{}'", SUBCMD, a),
            None => bail!("expected subcommand '{}'", SUBCMD),
        }
        let mut args = vec![];
        for arg in &mut raw_args {
            let arg = arg?;
            if arg == "--" {
                break;
            }
            args.push(arg);
        }
        let rest = raw_args.collect::<Result<Vec<_>>>()?;

        let mut cargo_args = vec![];
        let mut subcommand: Option<Subcommand> = None;

        let mut manifest_path = None;
        let mut color = None;

        let mut doctests = false;
        let mut no_run = false;
        let mut no_fail_fast = false;
        let mut ignore_run_fail = false;
        let mut lib = false;
        let mut bin = vec![];
        let mut bins = false;
        let mut example = vec![];
        let mut examples = false;
        let mut test = vec![];
        let mut tests = false;
        let mut bench = vec![];
        let mut benches = false;
        let mut all_targets = false;
        let mut doc = false;

        let mut package = vec![];
        let mut workspace = false;
        let mut exclude = vec![];
        let mut exclude_from_test = vec![];
        let mut exclude_from_report = vec![];

        // llvm-cov options
        let mut json = false;
        let mut lcov = false;
        let mut text = false;
        let mut html = false;
        let mut open = false;
        let mut summary_only = false;
        let mut output_path = None;
        let mut output_dir = None;
        let mut failure_mode = None;
        let mut ignore_filename_regex = None;
        let mut disable_default_ignore_filename_regex = false;
        let mut hide_instantiations = false;
        let mut no_cfg_coverage = false;
        let mut no_cfg_coverage_nightly = false;
        let mut no_report = false;
        let mut fail_under_lines = None;
        let mut fail_uncovered_lines = None;
        let mut fail_uncovered_regions = None;
        let mut fail_uncovered_functions = None;
        let mut show_missing_lines = false;
        let mut include_build_script = false;

        // build options
        let mut jobs = None;
        let mut release = false;
        let mut profile = None;
        let mut target = None;
        let mut coverage_target_only = false;
        let mut remap_path_prefix = false;
        let mut include_ffi = false;
        let mut verbose = 0;

        let mut parser = lexopt::Parser::from_args(args);
        while let Some(arg) = parser.next()? {
            macro_rules! parse_opt {
                ($opt:ident $(,)?) => {{
                    if $opt.is_some() {
                        multi_arg(&arg)?;
                    }
                    $opt = Some(parser.value()?.parse()?);
                }};
            }
            macro_rules! parse_flag {
                ($flag:ident $(,)?) => {
                    if mem::replace(&mut $flag, true) {
                        multi_arg(&arg)?;
                    }
                };
            }

            match arg {
                Long("color") => parse_opt!(color),
                Long("manifest-path") => parse_opt!(manifest_path),

                Long("doctests") => parse_flag!(doctests),
                Long("no-run") => parse_flag!(no_run),
                Long("no-fail-fast") => parse_flag!(no_fail_fast),
                Long("ignore-run-fail") => parse_flag!(ignore_run_fail),
                Long("lib") => parse_flag!(lib),
                Long("bin") => bin.push(parser.value()?.parse()?),
                Long("bins") => parse_flag!(bins),
                Long("example") => example.push(parser.value()?.parse()?),
                Long("examples") => parse_flag!(examples),
                Long("test") => test.push(parser.value()?.parse()?),
                Long("tests") => parse_flag!(tests),
                Long("bench") => bench.push(parser.value()?.parse()?),
                Long("benches") => parse_flag!(benches),
                Long("all-targets") => parse_flag!(all_targets),
                Long("doc") => parse_flag!(doc),

                Short('p') | Long("package") => package.push(parser.value()?.parse()?),
                Long("workspace" | "all") => parse_flag!(workspace),
                Long("exclude") => exclude.push(parser.value()?.parse()?),
                Long("exclude-from-test") => exclude_from_test.push(parser.value()?.parse()?),
                Long("exclude-from-report") => exclude_from_report.push(parser.value()?.parse()?),

                // llvm-cov options
                Long("json") => parse_flag!(json),
                Long("lcov") => parse_flag!(lcov),
                Long("text") => parse_flag!(text),
                Long("html") => parse_flag!(html),
                Long("open") => parse_flag!(open),
                Long("summary-only") => parse_flag!(summary_only),
                Long("output-path") => parse_opt!(output_path),
                Long("output-dir") => parse_opt!(output_dir),
                Long("failure-mode") => parse_opt!(failure_mode),
                Long("ignore-filename-regex") => parse_opt!(ignore_filename_regex),
                Long("disable-default-ignore-filename-regex") => {
                    parse_flag!(disable_default_ignore_filename_regex);
                }
                Long("hide-instantiations") => parse_flag!(hide_instantiations),
                Long("no-cfg-coverage") => parse_flag!(no_cfg_coverage),
                Long("no-cfg-coverage-nightly") => parse_flag!(no_cfg_coverage_nightly),
                Long("no-report") => parse_flag!(no_report),
                Long("fail-under-lines") => parse_opt!(fail_under_lines),
                Long("fail-uncovered-lines") => parse_opt!(fail_uncovered_lines),
                Long("fail-uncovered-regions") => parse_opt!(fail_uncovered_regions),
                Long("fail-uncovered-functions") => parse_opt!(fail_uncovered_functions),
                Long("show-missing-lines") => parse_flag!(show_missing_lines),
                Long("include-build-script") => parse_flag!(include_build_script),

                // build options
                Short('j') | Long("jobs") => parse_opt!(jobs),
                Short('r') | Long("release") => parse_flag!(release),
                Long("profile") => parse_opt!(profile),
                Long("target") => parse_opt!(target),
                Long("coverage-target-only") => parse_flag!(coverage_target_only),
                Long("remap-path-prefix") => parse_flag!(remap_path_prefix),
                Long("include-ffi") => parse_flag!(include_ffi),
                Short('v') | Long("verbose") => verbose += 1,

                Short('h') if subcommand.is_none() => {
                    // println!("{}", Help::short()); // TODO
                    std::process::exit(0);
                }
                Long("help") if subcommand.is_none() => {
                    // println!("{}", Help::long()); // TODO
                    std::process::exit(0);
                }
                Short('V') | Long("version") => {
                    if subcommand.is_none() {
                        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                        std::process::exit(0);
                    } else {
                        bail!("Found argument '--version' which wasn't expected, or isn't valid in this context");
                    }
                }

                // passthrough
                Long(flag) => {
                    let flag = format!("--{}", flag);
                    if let Some(val) = parser.optional_value() {
                        cargo_args.push(format!("{}={}", flag, val.parse::<String>()?));
                    } else {
                        cargo_args.push(flag);
                    }
                }
                Short(flag) => {
                    if matches!(flag, 'q' | 'r') {
                        // To handle combined short flags properly, handle known
                        // short flags without value as special cases.
                        cargo_args.push(format!("-{}", flag));
                    } else if let Some(val) = parser.optional_value() {
                        cargo_args.push(format!("-{}{}", flag, val.parse::<String>()?));
                    } else {
                        cargo_args.push(format!("-{}", flag));
                    }
                }
                Value(val) => {
                    let val = val.parse::<String>()?;
                    if subcommand.is_none() {
                        if let Ok(v) = val.parse() {
                            subcommand = Some(v);
                            if subcommand == Some(Subcommand::Demangle) {
                                if let Some(arg) = parser.next()? {
                                    return Err(arg.unexpected().into());
                                }
                            }
                        } else {
                            cargo_args.push(val);
                        }
                    } else {
                        cargo_args.push(val);
                    }
                }
            }
        }

        let subcommand = subcommand.unwrap_or(Subcommand::Test);

        // TODO
        // term::set_coloring(color.as_deref())?;

        if !exclude.is_empty() && !workspace {
            // TODO: This is the same behavior as cargo, but should we allow it to be used
            // in the root of a virtual workspace as well?
            requires("--exclude", &["--workspace"])?;
        }

        term::verbose::set(verbose != 0);
        // If `-vv` is passed, propagate `-v` to cargo.
        if verbose > 1 {
            cargo_args.push(format!("-{}", "v".repeat(verbose - 1)));
        }

        Ok(Self {
            subcommand,
            cov: LlvmCovOptions {
                json,
                lcov,
                text,
                html,
                open,
                summary_only,
                output_path,
                output_dir,
                failure_mode,
                ignore_filename_regex,
                disable_default_ignore_filename_regex,
                hide_instantiations,
                no_cfg_coverage,
                no_cfg_coverage_nightly,
                no_report,
                fail_under_lines,
                fail_uncovered_lines,
                fail_uncovered_regions,
                fail_uncovered_functions,
                show_missing_lines,
                include_build_script,
            },
            doctests,
            no_run,
            no_fail_fast,
            ignore_run_fail,
            lib,
            bin,
            bins,
            example,
            examples,
            test,
            tests,
            bench,
            benches,
            all_targets,
            doc,
            package,
            workspace,
            exclude,
            exclude_from_test,
            exclude_from_report,
            build: BuildOptions {
                jobs,
                release,
                profile,
                target,
                coverage_target_only,
                verbose: verbose.try_into().unwrap_or(u8::MAX),
                color,
                remap_path_prefix,
                include_ffi,
            },
            manifest: ManifestOptions { manifest_path },
            cargo_args,
            rest,
        })
    }

    pub(crate) fn cov(&mut self) -> LlvmCovOptions {
        mem::take(&mut self.cov)
    }

    pub(crate) fn build(&mut self) -> BuildOptions {
        mem::take(&mut self.build)
    }

    pub(crate) fn manifest(&mut self) -> ManifestOptions {
        mem::take(&mut self.manifest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subcommand {
    Test,

    // /// Run a binary or example and generate coverage report.
    // #[clap(
    //     bin_name = "cargo llvm-cov run",
    //     max_term_width(MAX_TERM_WIDTH),
    //     setting(AppSettings::DeriveDisplayOrder)
    // )]
    Run,

    /// Output the environment set by cargo-llvm-cov to build Rust projects.
    // #[clap(
    //     bin_name = "cargo llvm-cov show-env",
    //     max_term_width(MAX_TERM_WIDTH),
    //     setting(AppSettings::DeriveDisplayOrder)
    // )]
    ShowEnv, /* (ShowEnvOptions) */

    // /// Remove artifacts that cargo-llvm-cov has generated in the past
    // #[clap(
    //     bin_name = "cargo llvm-cov clean",
    //     max_term_width(MAX_TERM_WIDTH),
    //     setting(AppSettings::DeriveDisplayOrder)
    // )]
    Clean, /* (CleanOptions) */

    // /// Run tests with cargo nextest
    // #[clap(
    //     bin_name = "cargo llvm-cov nextest",
    //     max_term_width(MAX_TERM_WIDTH),
    //     setting(AppSettings::DeriveDisplayOrder),
    //     trailing_var_arg = true,
    //     allow_hyphen_values = true
    // )]
    Nextest, /*{
                 #[clap(multiple_values = true)]
                 passthrough_options: Vec<String>,
             }*/

    // internal (unstable)
    // #[clap(
    //     bin_name = "cargo llvm-cov demangle",
    //     max_term_width(MAX_TERM_WIDTH),
    //     hide = true,
    //     setting(AppSettings::DeriveDisplayOrder)
    // )]
    Demangle,
}

impl FromStr for Subcommand {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "test" | "t" => Ok(Subcommand::Test),
            "run" | "r" => Ok(Subcommand::Run),
            "show-env" => Ok(Subcommand::ShowEnv),
            "clean" => Ok(Subcommand::Clean),
            "nextest" => Ok(Subcommand::Nextest),
            "demangle" => Ok(Subcommand::Demangle),
            _ => bail!("unrecognized subcommand {}", s),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct LlvmCovOptions {
    /// Export coverage data in "json" format
    ///
    /// If --output-path is not specified, the report will be printed to stdout.
    ///
    /// This internally calls `llvm-cov export -format=text`.
    /// See <https://llvm.org/docs/CommandGuide/llvm-cov.html#llvm-cov-export> for more.
    // #[clap(long)]
    pub(crate) json: bool,
    /// Export coverage data in "lcov" format
    ///
    /// If --output-path is not specified, the report will be printed to stdout.
    ///
    /// This internally calls `llvm-cov export -format=lcov`.
    /// See <https://llvm.org/docs/CommandGuide/llvm-cov.html#llvm-cov-export> for more.
    // #[clap(long, conflicts_with = "json")]
    pub(crate) lcov: bool,

    /// Generate coverage report in “text” format
    ///
    /// If --output-path or --output-dir is not specified, the report will be printed to stdout.
    ///
    /// This internally calls `llvm-cov show -format=text`.
    /// See <https://llvm.org/docs/CommandGuide/llvm-cov.html#llvm-cov-show> for more.
    // #[clap(long, conflicts_with = "json", conflicts_with = "lcov")]
    pub(crate) text: bool,
    /// Generate coverage report in "html" format
    ///
    /// If --output-dir is not specified, the report will be generated in `target/llvm-cov/html` directory.
    ///
    /// This internally calls `llvm-cov show -format=html`.
    /// See <https://llvm.org/docs/CommandGuide/llvm-cov.html#llvm-cov-show> for more.
    // #[clap(long, conflicts_with = "json", conflicts_with = "lcov", conflicts_with = "text")]
    pub(crate) html: bool,
    /// Generate coverage reports in "html" format and open them in a browser after the operation.
    ///
    /// See --html for more.
    // #[clap(long, conflicts_with = "json", conflicts_with = "lcov", conflicts_with = "text")]
    pub(crate) open: bool,

    /// Export only summary information for each file in the coverage data
    ///
    /// This flag can only be used together with either --json or --lcov.
    // If the format flag is not specified, this flag is no-op because the only summary is displayed anyway.
    // #[clap(long, conflicts_with = "text", conflicts_with = "html", conflicts_with = "open")]
    pub(crate) summary_only: bool,
    /// Specify a file to write coverage data into.
    ///
    /// This flag can only be used together with --json, --lcov, or --text.
    /// See --output-dir for --html and --open.
    // #[clap(
    //     long,
    //     value_name = "PATH",
    //     conflicts_with = "html",
    //     conflicts_with = "open",
    //     forbid_empty_values = true
    // )]
    pub(crate) output_path: Option<Utf8PathBuf>,
    /// Specify a directory to write coverage report into (default to `target/llvm-cov`).
    ///
    /// This flag can only be used together with --text, --html, or --open.
    /// See also --output-path.
    // If the format flag is not specified, this flag is no-op.
    // #[clap(
    //     long,
    //     value_name = "DIRECTORY",
    //     conflicts_with = "json",
    //     conflicts_with = "lcov",
    //     conflicts_with = "output-path",
    //     forbid_empty_values = true
    // )]
    pub(crate) output_dir: Option<Utf8PathBuf>,

    /// Fail if `any` or `all` profiles cannot be merged (default to `any`)
    // #[clap(long, value_name = "any|all", possible_values(&["any", "all"]), hide_possible_values = true)]
    pub(crate) failure_mode: Option<String>,
    /// Skip source code files with file paths that match the given regular expression.
    // #[clap(long, value_name = "PATTERN", forbid_empty_values = true)]
    pub(crate) ignore_filename_regex: Option<String>,
    // For debugging (unstable)
    // #[clap(long, hide = true)]
    pub(crate) disable_default_ignore_filename_regex: bool,
    /// Hide instantiations from report
    // #[clap(long)]
    pub(crate) hide_instantiations: bool,
    /// Unset cfg(coverage), which is enabled when code is built using cargo-llvm-cov.
    // #[clap(long)]
    pub(crate) no_cfg_coverage: bool,
    /// Unset cfg(coverage_nightly), which is enabled when code is built using cargo-llvm-cov and nightly compiler.
    // #[clap(long)]
    pub(crate) no_cfg_coverage_nightly: bool,
    /// Run tests, but don't generate coverage report
    // #[clap(long)]
    pub(crate) no_report: bool,
    /// Exit with a status of 1 if the total line coverage is less than MIN percent.
    // #[clap(long, value_name = "MIN")]
    pub(crate) fail_under_lines: Option<f64>,
    /// Exit with a status of 1 if the uncovered lines are greater than MAX.
    // #[clap(long, value_name = "MAX")]
    pub(crate) fail_uncovered_lines: Option<u64>,
    /// Exit with a status of 1 if the uncovered regions are greater than MAX.
    // #[clap(long, value_name = "MAX")]
    pub(crate) fail_uncovered_regions: Option<u64>,
    /// Exit with a status of 1 if the uncovered functions are greater than MAX.
    // #[clap(long, value_name = "MAX")]
    pub(crate) fail_uncovered_functions: Option<u64>,
    /// Show lines with no coverage.
    // #[clap(long)]
    pub(crate) show_missing_lines: bool,
    /// Include build script in coverage report.
    // #[clap(long)]
    pub(crate) include_build_script: bool,
}

impl LlvmCovOptions {
    pub(crate) const fn show(&self) -> bool {
        self.text || self.html
    }
}

#[derive(Debug, Default)]
pub(crate) struct BuildOptions {
    // /// Number of parallel jobs, defaults to # of CPUs
    // // Max value is u32::MAX: https://github.com/rust-lang/cargo/blob/0.62.0/src/cargo/util/command_prelude.rs#L356
    // #[clap(short, long, value_name = "N")]
    pub(crate) jobs: Option<u32>,
    /// Build artifacts in release mode, with optimizations
    // #[clap(short, long)]
    pub(crate) release: bool,
    /// Build artifacts with the specified profile
    // #[clap(long, value_name = "PROFILE-NAME")]
    pub(crate) profile: Option<String>,
    // /// Space or comma separated list of features to activate
    // #[clap(short = 'F', long, multiple_occurrences = true, value_name = "FEATURES")]
    // pub(crate) features: Vec<String>,
    // /// Activate all available features
    // #[clap(long)]
    // pub(crate) all_features: bool,
    // /// Do not activate the `default` feature
    // #[clap(long)]
    // pub(crate) no_default_features: bool,
    /// Build for the target triple
    ///
    /// When this option is used, coverage for proc-macro and build script will
    /// not be displayed because cargo does not pass RUSTFLAGS to them.
    // #[clap(long, value_name = "TRIPLE")]
    pub(crate) target: Option<String>,
    /// Activate coverage reporting only for the target triple
    ///
    /// Activate coverage reporting only for the target triple specified via `--target`.
    /// This is important, if the project uses multiple targets via the cargo
    /// bindeps feature, and not all targets can use `instrument-coverage`,
    /// e.g. a microkernel, or an embedded binary.
    // #[clap(long, requires = "target")]
    pub(crate) coverage_target_only: bool,
    // TODO: Currently, we are using a subdirectory of the target directory as
    //       the actual target directory. What effect should this option have
    //       on its behavior?
    // /// Directory for all generated artifacts
    // #[clap(long, value_name = "DIRECTORY")]
    // target_dir: Option<Utf8PathBuf>,
    // /// Use verbose output
    // ///
    // /// Use -vv (-vvv) to propagate verbosity to cargo.
    // #[clap(short, long, parse(from_occurrences))]
    pub(crate) verbose: u8,
    /// Coloring
    // This flag will be propagated to both cargo and llvm-cov.
    // #[clap(long, arg_enum, value_name = "WHEN")]
    pub(crate) color: Option<Coloring>,

    /// Use --remap-path-prefix for workspace root
    ///
    /// Note that this does not fully compatible with doctest.
    // #[clap(long)]
    pub(crate) remap_path_prefix: bool,
    /// Include coverage of C/C++ code linked to Rust library/binary
    ///
    /// Note that `CC`/`CXX`/`LLVM_COV`/`LLVM_PROFDATA` environment variables
    /// must be set to Clang/LLVM compatible with the LLVM version used in rustc.
    // TODO: support specifying languages like: --include-ffi=c,  --include-ffi=c,c++
    // #[clap(long)]
    pub(crate) include_ffi: bool,
}

impl BuildOptions {
    pub(crate) fn cargo_args(&self, cmd: &mut ProcessBuilder) {
        if let Some(jobs) = self.jobs {
            cmd.arg("--jobs");
            cmd.arg(jobs.to_string());
        }
        if self.release {
            cmd.arg("--release");
        }
        if let Some(profile) = &self.profile {
            cmd.arg("--profile");
            cmd.arg(profile);
        }
        if let Some(target) = &self.target {
            cmd.arg("--target");
            cmd.arg(target);
        }

        if let Some(color) = self.color {
            cmd.arg("--color");
            cmd.arg(color.cargo_color());
        }
    }
}

/*
#[derive(Debug)]
pub(crate) struct RunOptions {
    // #[clap(flatten)]
    cov: LlvmCovOptions,

    /// No output printed to stdout
    // #[clap(short, long, conflicts_with = "verbose")]
    pub(crate) quiet: bool,
    /// Name of the bin target to run
    // #[clap(long, multiple_occurrences = true, value_name = "NAME")]
    pub(crate) bin: Vec<String>,
    /// Name of the example target to run
    // #[clap(long, multiple_occurrences = true, value_name = "NAME")]
    pub(crate) example: Vec<String>,
    /// Package with the target to run
    // #[clap(short, long, value_name = "SPEC")]
    pub(crate) package: Option<String>,

    // #[clap(flatten)]
    build: BuildOptions,

    // #[clap(flatten)]
    manifest: ManifestOptions,

    /// Unstable (nightly-only) flags to Cargo
    // #[clap(short = 'Z', multiple_occurrences = true, value_name = "FLAG")]
    pub(crate) unstable_flags: Vec<String>,

    /// Arguments for the test binary
    // #[clap(last = true)]
    pub(crate) args: Vec<String>,
}

impl RunOptions {
    pub(crate) fn cov(&mut self) -> LlvmCovOptions {
        mem::take(&mut self.cov)
    }

    pub(crate) fn build(&mut self) -> BuildOptions {
        mem::take(&mut self.build)
    }

    pub(crate) fn manifest(&mut self) -> ManifestOptions {
        mem::take(&mut self.manifest)
    }
}
*/

#[derive(Debug)]
pub(crate) struct ShowEnvOptions {
    /// Prepend "export " to each line, so that the output is suitable to be sourced by bash.
    // #[clap(long)]
    pub(crate) export_prefix: bool,
}

/*
#[derive(Debug)]
pub(crate) struct CleanOptions {
    /// Remove artifacts that may affect the coverage results of packages in the workspace.
    // #[clap(long)]
    pub(crate) workspace: bool,
    // TODO: Currently, we are using a subdirectory of the target directory as
    //       the actual target directory. What effect should this option have
    //       on its behavior?
    // /// Directory for all generated artifacts
    // #[clap(long, value_name = "DIRECTORY")]
    // pub(crate) target_dir: Option<Utf8PathBuf>,
    /// Use verbose output
    // #[clap(short, long, parse(from_occurrences))]
    pub(crate) verbose: u8,
    /// Coloring
    // #[clap(long, arg_enum, value_name = "WHEN")]
    pub(crate) color: Option<Coloring>,
    // #[clap(flatten)]
    pub(crate) manifest: ManifestOptions,
}
 */
// https://doc.rust-lang.org/nightly/cargo/commands/cargo-test.html#manifest-options
#[derive(Debug, Default)]
pub(crate) struct ManifestOptions {
    /// Path to Cargo.toml
    // #[clap(long, value_name = "PATH")]
    pub(crate) manifest_path: Option<Utf8PathBuf>,
}

fn format_flag(flag: &lexopt::Arg<'_>) -> String {
    match flag {
        Long(flag) => format!("--{}", flag),
        Short(flag) => format!("-{}", flag),
        Value(_) => unreachable!(),
    }
}

#[cold]
fn multi_arg(flag: &lexopt::Arg<'_>) -> Result<()> {
    let flag = &format_flag(flag);
    bail!("The argument '{}' was provided more than once, but cannot be used multiple times", flag);
}

// `flag` requires one of `requires`.
#[cold]
fn requires(flag: &str, requires: &[&str]) -> Result<()> {
    let with = match requires.len() {
        0 => unreachable!(),
        1 => requires[0].to_string(),
        2 => format!("either {} or {}", requires[0], requires[1]),
        _ => {
            let mut with = String::new();
            for f in requires.iter().take(requires.len() - 1) {
                with += f;
                with += ", ";
            }
            with += "or ";
            with += requires.last().unwrap();
            with
        }
    };
    bail!("{} can only be used together with {}", flag, with);
}

#[cold]
fn conflicts(a: &str, b: &str) -> Result<()> {
    bail!("{} may not be used together with {}", a, b);
}

#[cfg(test)]
mod tests {
    /*
    use std::{
        env,
        io::Write,
        panic,
        path::Path,
        process::{Command, Stdio},
    };

    use anyhow::Result;
    use fs_err as fs;

    use super::{Args, MAX_TERM_WIDTH};

    // https://github.com/clap-rs/clap/issues/751
    #[cfg(unix)]
    #[test]
    fn non_utf8_arg() {
        use std::{ffi::OsStr, os::unix::prelude::OsStrExt};

        // `cargo llvm-cov -- $'fo\x80o'`
        Opts::try_parse_from(&[
            "cargo".as_ref(),
            "llvm-cov".as_ref(),
            "--".as_ref(),
            OsStr::from_bytes(&[b'f', b'o', 0x80, b'o']),
        ])
        .unwrap_err();
    }

    // https://github.com/taiki-e/cargo-llvm-cov/pull/127#issuecomment-1018204521
    #[test]
    fn multiple_values() {
        Opts::try_parse_from(&["cargo", "llvm-cov", "--features", "a", "b"]).unwrap_err();
        Opts::try_parse_from(&["cargo", "llvm-cov", "--package", "a", "b"]).unwrap_err();
        Opts::try_parse_from(&["cargo", "llvm-cov", "--exclude", "a", "b"]).unwrap_err();
        Opts::try_parse_from(&["cargo", "llvm-cov", "-Z", "a", "b"]).unwrap_err();
    }

    // https://github.com/clap-rs/clap/issues/1740
    #[test]
    fn empty_value() {
        let forbidden = &[
            "--output-path",
            "--output-dir",
            "--ignore-filename-regex",
            // "--target-dir",
        ];
        let allowed = &[
            "--bin",
            "--example",
            "--test",
            "--bench",
            "--package",
            "--exclude",
            "--profile",
            "--features",
            "--target",
            // "--target-dir",
            "--manifest-path",
            "-Z",
            "--",
        ];

        for &flag in forbidden {
            Opts::try_parse_from(&["cargo", "llvm-cov", flag, ""]).unwrap_err();
        }
        for &flag in allowed {
            if flag == "--exclude" {
                Opts::try_parse_from(&["cargo", "llvm-cov", flag, "", "--workspace"]).unwrap();
            } else {
                Opts::try_parse_from(&["cargo", "llvm-cov", flag, ""]).unwrap();
            }
        }
    }

    fn get_help(long: bool) -> Result<String> {
        let mut buf = vec![];
        if long {
            Args::command().term_width(MAX_TERM_WIDTH).write_long_help(&mut buf)?;
        } else {
            Args::command().term_width(MAX_TERM_WIDTH).write_help(&mut buf)?;
        }
        let mut out = String::new();
        for mut line in String::from_utf8(buf)?.lines() {
            if let Some(new) = line.trim_end().strip_suffix(env!("CARGO_PKG_VERSION")) {
                line = new;
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        Ok(out)
    }

    #[track_caller]
    fn assert_diff(expected_path: impl AsRef<Path>, actual: impl AsRef<str>) {
        let actual = actual.as_ref();
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest_dir =
            manifest_dir.strip_prefix(env::current_dir().unwrap()).unwrap_or(manifest_dir);
        let expected_path = &manifest_dir.join(expected_path);
        if !expected_path.is_file() {
            fs::write(expected_path, "").unwrap();
        }
        let expected = fs::read_to_string(expected_path).unwrap();
        if expected != actual {
            if env::var_os("CI").is_some() {
                let mut child = Command::new("git")
                    .args(["--no-pager", "diff", "--no-index", "--"])
                    .arg(expected_path)
                    .arg("-")
                    .stdin(Stdio::piped())
                    .spawn()
                    .unwrap();
                child.stdin.as_mut().unwrap().write_all(actual.as_bytes()).unwrap();
                assert!(!child.wait().unwrap().success());
                // patch -p1 <<'EOF' ... EOF
                panic!("assertion failed; please run test locally and commit resulting changes, or apply above diff as patch");
            } else {
                fs::write(expected_path, actual).unwrap();
            }
        }
    }

    #[test]
    fn long_help() {
        let actual = get_help(true).unwrap();
        assert_diff("tests/long-help.txt", actual);
    }

    #[test]
    fn short_help() {
        let actual = get_help(false).unwrap();
        assert_diff("tests/short-help.txt", actual);
    }

    #[test]
    fn update_readme() -> Result<()> {
        let new = get_help(true)?;
        let path = &Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let base = fs::read_to_string(path)?;
        let mut out = String::with_capacity(base.capacity());
        let mut lines = base.lines();
        let mut start = false;
        let mut end = false;
        while let Some(line) = lines.next() {
            out.push_str(line);
            out.push('\n');
            if line == "<!-- readme-long-help:start -->" {
                start = true;
                out.push_str("```console\n");
                out.push_str("$ cargo llvm-cov --help\n");
                out.push_str(&new);
                for line in &mut lines {
                    if line == "<!-- readme-long-help:end -->" {
                        out.push_str("```\n");
                        out.push_str(line);
                        out.push('\n');
                        end = true;
                        break;
                    }
                }
            }
        }
        if start && end {
            assert_diff(path, out);
        } else if start {
            panic!("missing `<!-- readme-long-help:end -->` comment in README.md");
        } else {
            panic!("missing `<!-- readme-long-help:start -->` comment in README.md");
        }
        Ok(())
    }
    */
}
