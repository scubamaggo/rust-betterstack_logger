use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::{collections::HashMap, str::FromStr};
use reqwest::Client;
#[cfg(feature = "timestamps")]
use time::{format_description::FormatItem, OffsetDateTime, UtcOffset};


#[cfg(feature = "timestamps")]
const TIMESTAMP_FORMAT_OFFSET: &[FormatItem] = time::macros::format_description!(
    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3][offset_hour sign:mandatory]:[offset_minute]"
);

#[cfg(feature = "timestamps")]
const TIMESTAMP_FORMAT_UTC: &[FormatItem] =
    time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

#[cfg(feature = "timestamps")]
#[derive(PartialEq)]
enum Timestamps {
    None,
    Local,
    Utc,
    UtcOffset(UtcOffset),
}

/// Implements [`Log`] and a set of simple builder methods for configuration.
///
/// Use the various "builder" methods on this struct to configure the logger,
/// then call [`init`] to configure the [`log`] crate.
pub struct BetterStackLogger {
    /// The default logging level
    default_level: LevelFilter,

    /// The specific logging level for each module
    ///
    /// This is used to override the default value for some specific modules.
    ///
    /// This must be sorted from most-specific to least-specific, so that [`enabled`](#method.enabled) can scan the
    /// vector for the first match to give us the desired log level for a module.
    module_levels: Vec<(String, LevelFilter)>,

    client: Client,
    source_token: String,

    /// Whether to include thread names (and IDs) or not
    ///
    /// This field is only available if the `threads` feature is enabled.
    #[cfg(feature = "threads")]
    threads: bool,

    /// Control how timestamps are displayed.
    ///
    /// This field is only available if the `timestamps` feature is enabled.
    #[cfg(feature = "timestamps")]
    timestamps: Timestamps,
    #[cfg(feature = "timestamps")]
    timestamps_format: Option<&'static [FormatItem<'static>]>,
}


impl BetterStackLogger {
    /// Initializes the global logger with a BetterStackLogger instance with
    /// default log level set to `Level::Trace`.
    ///
    /// ```no_run
    /// use betterstack_logger::BetterStackLogger;
    /// BetterStackLogger::new().env().init().unwrap();
    /// log::warn!("This is an example message.");
    /// ```
    ///
    /// [`init`]: #method.init
    #[must_use = "You must call init() to begin logging"]
    pub fn new(source_token: &str) -> BetterStackLogger {
        BetterStackLogger {
            default_level: LevelFilter::Trace,
            module_levels: Vec::new(),

            #[cfg(feature = "threads")]
            threads: false,

            #[cfg(feature = "timestamps")]
            timestamps: Timestamps::Utc,

            #[cfg(feature = "timestamps")]
            timestamps_format: None,

            client: Client::new(),
            source_token: source_token.to_string(),     
        }
    }

    /// Enables the user to choose log level by setting `RUST_LOG=<level>`
    /// environment variable. This will use the default level set by
    /// [`with_level`] if `RUST_LOG` is not set or can't be parsed as a
    /// standard log level.
    ///dd
    /// This must be called after [`with_level`]. If called before
    /// [`with_level`], it will have no effect.
    ///
    /// ```no_run
    /// use betterstack_logger::BetterStackLogger;
    /// BetterStackLogger::env().init(source_token).unwrap();
    /// log::warn!("This is an example message.");
    /// ```
    /// 
    /// [`with_level`]: #method.with_level
    #[must_use = "You must call init() to begin logging"]
    pub fn env(mut self) -> BetterStackLogger {
        self.default_level = std::env::var("RUST_LOG")
            .ok()
            .as_deref()
            .map(log::LevelFilter::from_str)
            .and_then(Result::ok)
            .unwrap_or(self.default_level);

        self
    }

    /// Set the 'default' log level.
    ///
    /// You can override the default level for specific modules and their sub-modules using [`with_module_level`]
    ///
    /// This must be called before [`env`]. If called after [`env`], it will override the value loaded from the environment.
    ///
    /// [`env`]: #method.env
    /// [`with_module_level`]: #method.with_module_level
    #[must_use = "You must call init() to begin logging"]
    pub fn with_level(mut self, level: LevelFilter) -> BetterStackLogger {
        self.default_level = level;
        self
    }

    /// Override the log level for some specific modules.
    ///
    /// This sets the log level of a specific module and all its sub-modules.
    /// When both the level for a parent module as well as a child module are set,
    /// the more specific value is taken. If the log level for the same module is
    /// specified twice, the resulting log level is implementation defined.
    ///
    /// # Examples
    ///
    /// Silence an overly verbose crate:
    ///
    /// ```no_run
    /// use betterstack_logger::BetterStackLogger;
    /// use log::LevelFilter;
    ///
    /// BetterStackLogger::new().with_module_level("chatty_dependency", LevelFilter::Warn).init().unwrap();
    /// ```
    ///
    /// Disable logging for all dependencies:
    ///
    /// ```no_run
    /// use betterstack_logger::BetterStackLogger;
    /// use log::LevelFilter;
    ///
    /// BetterStackLogger::new()
    ///     .with_level(LevelFilter::Off)
    ///     .with_module_level("my_crate", LevelFilter::Info)
    ///     .init()
    ///     .unwrap();
    /// ```
    //
    // This method *must* sort `module_levels` for the [`enabled`](#method.enabled) method to work correctly.
    #[must_use = "You must call init() to begin logging"]
    pub fn with_module_level(mut self, target: &str, level: LevelFilter) -> BetterStackLogger {
        self.module_levels.push((target.to_string(), level));
        self.module_levels
            .sort_by_key(|(name, _level)| name.len().wrapping_neg());
        self
    }

    /// Override the log level for specific targets.
    // This method *must* sort `module_levels` for the [`enabled`](#method.enabled) method to work correctly.
    #[must_use = "You must call init() to begin logging"]
    #[deprecated(
        since = "1.11.0",
        note = "Use [`with_module_level`](#method.with_module_level) instead. Will be removed in version 2.0.0."
    )]
    pub fn with_target_levels(mut self, target_levels: HashMap<String, LevelFilter>) -> BetterStackLogger {
        self.module_levels = target_levels.into_iter().collect();
        self.module_levels
            .sort_by_key(|(name, _level)| name.len().wrapping_neg());
        self
    }

    /// Control whether thread names (and IDs) are printed or not.
    ///
    /// This method is only available if the `threads` feature is enabled.
    /// Thread names are disabled by default.
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "threads")]
    pub fn with_threads(mut self, threads: bool) -> BetterStackLogger {
        self.threads = threads;
        self
    }

    /// Control whether timestamps are printed or not.
    ///
    /// Timestamps will be displayed in the local timezone.
    ///
    /// This method is only available if the `timestamps` feature is enabled.
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "timestamps")]
    #[deprecated(
        since = "1.16.0",
        note = "Use [`with_local_timestamps`] or [`with_utc_timestamps`] instead. Will be removed in version 2.0.0."
    )]
    pub fn with_timestamps(mut self, timestamps: bool) -> BetterStackLogger {
        if timestamps {
            self.timestamps = Timestamps::Local
        } else {
            self.timestamps = Timestamps::None
        }
        self
    }

    /// Control the format used for timestamps.
    ///
    /// Without this, a default format is used depending on the timestamps type.
    ///
    /// The syntax for the format_description macro can be found in the
    /// [`time` crate book](https://time-rs.github.io/book/api/format-description.html).
    ///
    /// ```
    /// betterstack_logger::BetterStackLogger::new()
    ///  .with_level(log::LevelFilter::Debug)
    ///  .env()
    ///  .with_timestamp_format(time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"))
    ///  .init()
    ///  .unwrap();
    /// ```
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "timestamps")]
    pub fn with_timestamp_format(mut self, format: &'static [FormatItem<'static>]) -> BetterStackLogger {
        self.timestamps_format = Some(format);
        self
    }

    /// Don't display any timestamps.
    ///
    /// This method is only available if the `timestamps` feature is enabled.
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "timestamps")]
    pub fn without_timestamps(mut self) -> BetterStackLogger {
        self.timestamps = Timestamps::None;
        self
    }

    /// Display timestamps using the local timezone.
    ///
    /// This method is only available if the `timestamps` feature is enabled.
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "timestamps")]
    pub fn with_local_timestamps(mut self) -> BetterStackLogger {
        self.timestamps = Timestamps::Local;
        self
    }

    /// Display timestamps using UTC.
    ///
    /// This method is only available if the `timestamps` feature is enabled.
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "timestamps")]
    pub fn with_utc_timestamps(mut self) -> BetterStackLogger {
        self.timestamps = Timestamps::Utc;
        self
    }

    /// Display timestamps using a static UTC offset.
    ///
    /// This method is only available if the `timestamps` feature is enabled.
    #[must_use = "You must call init() to begin logging"]
    #[cfg(feature = "timestamps")]
    pub fn with_utc_offset(mut self, offset: UtcOffset) -> BetterStackLogger {
        self.timestamps = Timestamps::UtcOffset(offset);
        self
    }

    /// Configure the logger
    pub fn max_level(&self) -> LevelFilter {
        let max_level = self.module_levels.iter().map(|(_name, level)| level).copied().max();
        max_level
            .map(|lvl| lvl.max(self.default_level))
            .unwrap_or(self.default_level)
    }

    /// 'Init' the actual logger and instantiate it,
    /// this method MUST be called in order for the logger to be effective.
    pub fn init(self) -> Result<(), SetLoggerError> {
        #[cfg(all(windows, feature = "colored"))]
        set_up_windows_color_terminal();

        #[cfg(all(feature = "colored", feature = "stderr"))]
        use_stderr_for_colors();

        log::set_max_level(self.max_level());
        log::set_boxed_logger(Box::new(self))
    }
}

impl Log for BetterStackLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        &metadata.level().to_level_filter()
            <= self
                .module_levels
                .iter()
                /* At this point the Vec is already sorted so that we can simply take
                 * the first match
                 */
                .find(|(name, _level)| metadata.target().starts_with(name))
                .map(|(_name, level)| level)
                .unwrap_or(&self.default_level)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            
            let log_message = build_log_message(record);
            let body = serde_json::to_string(&log_message).unwrap();

            let client = self.client.clone(); 
            let source_token = self.source_token.clone(); 

            // Asynchronously send log message to BetterStack API
            tokio::spawn(async move {
                let _ = client
                    .post("https://api.betterstack.com/logs")
                    .header("Authorization", format!("Bearer {}", source_token))
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await;
                // Handle errors and retries if necessary
            });
        }
    }    

    fn flush(&self) {}
}

fn build_log_message(record: &Record) -> String {
    let level_string = format!("{:<5}", record.level().to_string());

    let target = if !record.target().is_empty() {
        record.target()
    } else {
        record.module_path().unwrap_or_default()
    };

    let thread = {
        #[cfg(feature = "threads")]
        if self.threads {
            let thread = std::thread::current();

            format!("@{}", {
                #[cfg(feature = "nightly")]
                {
                    thread.name().unwrap_or(&thread.id().as_u64().to_string())
                }

                #[cfg(not(feature = "nightly"))]
                {
                    thread.name().unwrap_or("?")
                }
            })
        } else {
            "".to_string()
        }

        #[cfg(not(feature = "threads"))]
        ""
    };

    let timestamp = {
        #[cfg(feature = "timestamps")]
        match self.timestamps {
            Timestamps::None => "".to_string(),
            Timestamps::Local => format!(
                "{} ",
                OffsetDateTime::now_local()
                    .expect(concat!(
                        "Could not determine the UTC offset on this system. ",
                        "Consider displaying UTC time instead. ",
                        "Possible causes are that the time crate does not implement \"local_offset_at\" ",
                        "on your system, or that you are running in a multi-threaded environment and ",
                        "the time crate is returning \"None\" from \"local_offset_at\" to avoid unsafe ",
                        "behaviour. See the time crate's documentation for more information. ",
                        "(https://time-rs.github.io/internal-api/time/index.html#feature-flags)"
                    ))
                    .format(&self.timestamps_format.unwrap_or(TIMESTAMP_FORMAT_OFFSET))
                    .unwrap()
            ),
            Timestamps::Utc => format!(
                "{} ",
                OffsetDateTime::now_utc()
                    .format(&self.timestamps_format.unwrap_or(TIMESTAMP_FORMAT_UTC))
                    .unwrap()
            ),
            Timestamps::UtcOffset(offset) => format!(
                "{} ",
                OffsetDateTime::now_utc()
                    .to_offset(offset)
                    .format(&self.timestamps_format.unwrap_or(TIMESTAMP_FORMAT_OFFSET))
                    .unwrap()
            ),
        }

        #[cfg(not(feature = "timestamps"))]
        ""
    };

    let message = format!("{}{} [{}{}] {}", timestamp, level_string, target, thread, record.args());
    message
}
/*
impl Log for BetterStackLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Format log message
            let log_message = format!(
                "[{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
            let body = serde_json::to_string(&log_message).unwrap();

            // Asynchronously send log message to BetterStack API
            tokio::spawn(async move {
                let _ = self
                    .client
                    .post("https://api.betterstack.com/logs")
                    .header("Authorization", format!("Bearer {}", self.source_token))
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await;
                // Handle errors and retries if necessary
            });
        }
    }

    fn flush(&self) {}
} */ 

// Initialise the logger with its default configuration.
///
/// Log messages will not be filtered.
/// The `RUST_LOG` environment variable is not used.
pub fn init(source_token: &str) -> Result<(), SetLoggerError> {
    BetterStackLogger::new(source_token).init()
}

/// Initialise the logger with its default configuration.
///
/// Log messages will not be filtered.
/// The `RUST_LOG` environment variable is not used.
///
/// This function is only available if the `timestamps` feature is enabled.
#[cfg(feature = "timestamps")]
pub fn init_utc(source_token: &str) -> Result<(), SetLoggerError> {
    BetterStackLogger::new(source_token).with_utc_timestamps().init()
}

/// Initialise the logger with the `RUST_LOG` environment variable.
///
/// Log messages will be filtered based on the `RUST_LOG` environment variable.
pub fn init_with_env(source_token: &str) -> Result<(), SetLoggerError> {
    BetterStackLogger::new(source_token).env().init()
}

/// Initialise the logger with a specific log level.
///
/// Log messages below the given [`Level`] will be filtered.
/// The `RUST_LOG` environment variable is not used.
pub fn init_with_level(source_token: &str, level: Level) -> Result<(), SetLoggerError> {
    BetterStackLogger::new(source_token).with_level(level.to_level_filter()).init()
}