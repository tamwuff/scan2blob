fn main() -> Result<(), scan2blob::error::WuffError> {
    let cmdline_parser: clap::Command =
        scan2blob::util::make_cmdline_parser("scan2blob-mkpass");
    let _cmdline_matches: clap::ArgMatches = cmdline_parser.get_matches();

    println!(
        "Warning: the password you enter will be echoed (by this tool) and will also be"
    );
    println!(
        "used as HTTP Basic Auth credentials (by the WebDAV server). HTTP Basic Auth is"
    );
    println!(
        "not a particularly secure authentication method. The point of all of this is,"
    );
    println!("please do not use a sensitive password.");
    println!();
    print!("Enter plaintext password: ");
    std::io::Write::flush(&mut std::io::stdout().lock()).expect("stdout");

    let mut plaintext: String = String::new();
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut plaintext)
        .expect("stdin");
    plaintext = String::from(plaintext.trim_end_matches(&['\r', '\n']));

    let password: String = scan2blob::pwhash::crypt(&plaintext);
    println!();
    println!("Hashed password: {}", password);
    println!();

    Ok(())
}
