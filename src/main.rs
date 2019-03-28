macro_rules! info {
    ( $quiet:expr, $($args:expr),+ ) => {
        if !($quiet) {
            println!($($args),*);
        }
    }
}

struct StringReplacer {
    search_expression: Option<regex::Regex>,
    replace_pattern: Option<String>,
}

impl StringReplacer {
    pub fn new(
        search_expression: Option<regex::Regex>,
        replace_pattern: Option<String>,
    ) -> StringReplacer {
        // if no search but there is a replace we'll need a basic search
        if search_expression.is_none() && replace_pattern.is_some() {
            StringReplacer {
                search_expression: Some(
                    regex::Regex::new(".*").expect("Failed to compile simple '.*' expression"),
                ),
                replace_pattern,
            }
        } else {
            StringReplacer {
                search_expression,
                replace_pattern,
            }
        }
    }

    pub fn matches(&self, text: &str) -> bool {
        match &self.search_expression {
            Some(expression) => expression.is_match(text),
            None => true,
        }
    }

    pub fn has_search(&self) -> bool {
        return self.search_expression.is_some();
    }

    pub fn has_replace(&self) -> bool {
        return self.replace_pattern.is_some();
    }

    pub fn do_replace<'t>(&self, text: &'t str) -> std::borrow::Cow<'t, str> {
        match &self.search_expression {
            Some(search) => match &self.replace_pattern {
                Some(replace) => search.replace_all(text, replace.as_str()),
                None => std::borrow::Cow::from(text),
            },
            None => std::borrow::Cow::from(text),
        }
    }
}

struct RSRInstance {
    filename_replacer: StringReplacer,
    text_replacer: StringReplacer,
    prompt: bool,
    quiet: bool,
}

impl RSRInstance {
    pub fn new(
        filename_replacer: StringReplacer,
        text_replacer: StringReplacer,
        prompt: bool,
        quiet: bool,
    ) -> RSRInstance {
        RSRInstance {
            filename_replacer,
            text_replacer,
            prompt,
            quiet,
        }
    }

    pub fn handle_directory(&self, directory: &std::path::Path) {
        match directory.read_dir() {
            Ok(iter) => {
                for entry in iter {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if let Ok(file_type) = entry.file_type() {
                            if file_type.is_dir() {
                                self.handle_directory(&path);
                            } else {
                                self.handle_file(&path);
                            }
                        } else {
                            info!(self.quiet, "Ignored {:?}, could not get file type", path);
                        }
                    } else {
                        info!(self.quiet, "Ignoring invalid entry within {:?}", directory);
                    }
                }
            }
            Err(e) => info!(
                self.quiet,
                "Skipping {:?}, error iterating directory: {}", directory, e
            ),
        }
    }

    fn handle_file(&self, file: &std::path::Path) {
        if let Some(filename) = file.file_name() {
            if let Some(filename) = filename.to_str() {
                if self.filename_replacer.matches(&filename) {
                    let mut print_filename = true;
                    if self.text_replacer.has_replace() {
                        print_filename = false; // did something so no need to print filename
                        self.replace_file_contents(&filename, &file);
                    } else if self.text_replacer.has_search() {
                        print_filename = false; // did something so no need to print filename
                        self.search_file_contents(&file);
                    }

                    // do we need to rename?
                    if self.filename_replacer.has_replace() {
                        let new_filename = self.filename_replacer.do_replace(filename);
                        let new_path = file.with_file_name(new_filename.as_ref());

                        if new_path != file {
                            print_filename = false; // did something so no need to print filename

                            if self.confirm(&format!("Rename {:?} => {:?}?", file, new_path)) {
                                if let Err(e) = std::fs::rename(file, &new_path) {
                                    println!(
                                        "Failed to rename {:?} to {:?}: {}!",
                                        file, new_path, e
                                    );
                                };
                            }
                        }
                    }

                    // if we didn't do text search or a rename it's just file match
                    if print_filename {
                        info!(self.quiet, "{}", file.to_string_lossy());
                    }
                }
            } else {
                info!(
                    self.quiet,
                    "Skipping {:?} as the the filename could not be parsed", file
                );
            }
        } else {
            info!(self.quiet, "Skipping {:?} as the it had no filename", file);
        }
    }

    fn replace_file_contents(&self, input_filename: &str, input_path: &std::path::Path) {
        let mut read_option = std::fs::OpenOptions::new();
        read_option.read(true);

        if let Ok(input_file) = read_option.open(&input_path) {
            let tmp_file = input_path.with_file_name(input_filename.to_owned() + ".rsr_tmp");

            let mut write_option = std::fs::OpenOptions::new();
            write_option.write(true).create_new(true);

            match write_option.open(&tmp_file) {
                Ok(output_file) => {
                    use std::io::{BufRead, Write};

                    let mut reader = std::io::BufReader::new(input_file);
                    let mut writer = std::io::BufWriter::new(output_file);
                    let mut line_number = 1;
                    loop {
                        line_number += 1; // starts at zero so increment first
                        let mut line = String::new();

                        match reader.read_line(&mut line) {
                            Ok(count) => {
                                // 0 count indicates we've read everything
                                if count == 0 {
                                    break;
                                }

                                let new_line = self.text_replacer.do_replace(&line);
                                let result = if new_line != line
                                    && self.confirm(&format!(
                                        "{}:{}\n\t{}\n\t=>\n\t{}",
                                        input_path.to_string_lossy(),
                                        line_number,
                                        line.trim(),
                                        new_line.trim()
                                    )) {
                                    writer.write_all(new_line.as_bytes())
                                } else {
                                    writer.write_all(line.as_bytes())
                                };

                                if let Err(e) = result {
                                    // this is actually an error, print regardless of quiet level
                                    println!(
                                        "Skipping {:?} as not all lines could be written to {:?}: {}",
                                        input_path, tmp_file, e
                                    );
                                    std::fs::remove_file(tmp_file).unwrap_or(()); // we don't care if the remove fails
                                    return;
                                }
                            }
                            Err(e) => {
                                // this is actually an error, print regardless of quiet level
                                println!(
                                    "Skipping {:?} as not all lines could be read: {}",
                                    input_path, e
                                );
                                std::fs::remove_file(tmp_file).unwrap_or(()); // we don't care if the remove fails
                                return;
                            }
                        }
                    }

                    // if we got here we've successfully read and written everything, close the files and rename the temp
                    drop(reader);
                    drop(writer);

                    if let Ok(old_metadata) = std::fs::metadata(&input_path) {
                        if let Err(e) =
                            std::fs::set_permissions(&tmp_file, old_metadata.permissions())
                        {
                            println!("Failed to match permissions for {:?}, permissions may have changed: {}", input_path, e);
                        }
                    }

                    if let Err(e) = std::fs::rename(&tmp_file, &input_path) {
                        // this is actually an error, print regardless of quiet level
                        println!(
                            "Failed to rename temporary file {:?} to original file {:?}: {}",
                            tmp_file, input_path, e
                        );
                    }
                }
                Err(e) => {
                    // this is actually an error, print regardless of quiet level
                    println!(
                        "Skipping {:?} as the the temporary file {:?} could not be opened: {}",
                        input_path, tmp_file, e
                    );
                }
            }
        } else {
            info!(
                self.quiet,
                "Skipping {:?} as the the file could not be opened", input_path
            );
        }
    }

    fn search_file_contents(&self, input_path: &std::path::Path) {
        let mut read_option = std::fs::OpenOptions::new();
        read_option.read(true);

        if let Ok(input_file) = read_option.open(&input_path) {
            use std::io::BufRead;

            let mut reader = std::io::BufReader::new(input_file);
            let mut line_number = 0;
            loop {
                line_number += 1; // starts at zero so increment first
                let mut line = String::new();

                match reader.read_line(&mut line) {
                    Ok(count) => {
                        // 0 count indicates we've read everything
                        if count == 0 {
                            break;
                        }

                        if self.text_replacer.matches(&line) {
                            info!(
                                self.quiet,
                                "{}:{: <8}{}",
                                input_path.to_string_lossy(),
                                line_number,
                                line.trim()
                            );
                        }
                    }
                    Err(e) => {
                        // this is actually an error, print regardless of quiet level
                        println!(
                            "Skipping {:?} as not all lines could be read: {}",
                            input_path, e
                        );
                        return;
                    }
                }
            }

            // if we got here we've successfully read and written everything, close the files and rename the temp
            drop(reader);
        } else {
            info!(
                self.quiet,
                "Skipping {:?} as the the file could not be opened", input_path
            );
        }
    }

    fn confirm(&self, message: &str) -> bool {
        match self.prompt {
            true => {
                println!("{} ... Confirm [y/N]: ", message);
                let mut user_response = String::new();
                match std::io::stdin().read_line(&mut user_response) {
                    Ok(_) => user_response.trim() == "y",
                    Err(_) => false,
                }
            }
            false => true,
        }
    }
}

fn main() {
    let args = clap::App::new("Recursive Search & Replace")
        .version("0.1.0")
        .about("A Recursive Search & Replace program which can find all files matching a pattern and find matches of another pattern in those files and potentially replace those as well")
        .author("Kevin Logan")
        .arg(clap::Arg::with_name("input")
            .short("i")
            .long("input")
            .required(false)
            .help("A regex pattern to filter files which will be included")
            .takes_value(true))
        .arg(clap::Arg::with_name("output")
            .short("o")
            .long("output")
            .required(false)
            .help("A replacement pattern to be applied to <input> (or '.*' if <input> is not provided) to produce the output filename")
            .takes_value(true))
        .arg(clap::Arg::with_name("search")
            .short("s")
            .long("search")
            .required(false)
            .help("A regex pattern for text to search for in the searched files")
            .takes_value(true))
        .arg(clap::Arg::with_name("replace")
            .short("r")
            .long("replace")
            .required(false)
            .help("A replacement pattern to replace any matching text with again <search>. May include references to capture groups, e.g. ${1} or named capture groups like ${name} which would be captured as (?P<name>.*). The curly-brackets are optional but may be required to distinguish between the capture and the rest of the replacement text")
            .takes_value(true))
        .arg(clap::Arg::with_name("prompt")
            .short("p")
            .long("prompt")
            .required(false)
            .help("If set, a y/N prompt will allow the user to decide if each instance of the found text should be replaced. Only relevant if <replace_pattern> is used")
            .takes_value(false))
        .arg(clap::Arg::with_name("quiet")
            .short("q")
            .long("quiet")
            .required(false)
            .help("If set, supresses any messages that are neither required nor errors")
            .takes_value(false))
        .arg(clap::Arg::with_name("dir")
            .required(false)
            .index(1)
            .help("The directory to search for files within"))
        .get_matches();

    let dir = match args.value_of("dir") {
        Some(value) => std::path::Path::new(value),
        None => std::path::Path::new("."),
    };
    let input = match args.value_of("input") {
        Some(pattern) => match regex::Regex::new(&pattern) {
            Ok(regex) => Some(regex),
            Err(e) => {
                println!("Failed to compile regex {}: {}", pattern, e);
                None
            }
        },
        None => None,
    };
    let output = match args.value_of("output") {
        Some(value) => Some(String::from(value)),
        None => None,
    };
    let search = match args.value_of("search") {
        Some(pattern) => match regex::Regex::new(&pattern) {
            Ok(regex) => Some(regex),
            Err(e) => {
                println!("Failed to compile regex {}: {}", pattern, e);
                None
            }
        },
        None => None,
    };
    let replace = match args.value_of("replace") {
        Some(value) => Some(String::from(value)),
        None => None,
    };
    let prompt = args.is_present("prompt");
    let quiet = args.is_present("quiet");

    let filename_replace = StringReplacer::new(input, output);
    let text_replace = StringReplacer::new(search, replace);

    let instance = RSRInstance::new(filename_replace, text_replace, prompt, quiet);

    instance.handle_directory(dir);
}
