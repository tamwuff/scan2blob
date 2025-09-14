fn confirm_inability_to_guarantee_prefix(
    prefix: &str,
    directory: Option<&str>,
) -> Result<(), scan2blob::error::WuffError> {
    println!("WARNING: I cannot guarantee a prefix of {}", prefix);
    if let Some(directory) = directory {
        println!("I can only guarantee a prefix of {}/", directory);
    }
    print!(
        "Is this ok? Type the word \"yes\" to proceed, anything else to cancel: "
    );
    std::io::Write::flush(&mut std::io::stdout().lock()).expect("stdout");
    let mut response: String = String::new();
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut response)
        .expect("stdin");
    if response.trim() == "yes" {
        Ok(())
    } else {
        Err(scan2blob::error::WuffError::from("Cancelling"))
    }
}

fn main() -> Result<(), scan2blob::error::WuffError> {
    let cmdline_parser: clap::Command =
        scan2blob::util::make_cmdline_parser("scan2blob-mksas")
            .arg(
                clap::Arg::new("storage_account")
                    .long("storage-account")
                    .required(true)
                    .action(clap::ArgAction::Set),
            )
            .arg(
                clap::Arg::new("container")
                    .long("container")
                    .required(true)
                    .action(clap::ArgAction::Set),
            )
            .arg(
                clap::Arg::new("prefix")
                    .long("prefix")
                    .default_value("")
                    .action(clap::ArgAction::Set),
            );

    let cmdline_matches: clap::ArgMatches = cmdline_parser.get_matches();
    let storage_account: &String = cmdline_matches
        .get_one::<String>("storage_account")
        .unwrap();
    let container: &String =
        cmdline_matches.get_one::<String>("container").unwrap();
    let prefix: &String = cmdline_matches.get_one::<String>("prefix").unwrap();
    let directory: Option<&str> = if prefix.is_empty() {
        None
    } else {
        if prefix.ends_with('/') {
            Some(&prefix[..(prefix.len() - 1)])
        } else if let Some((directory, _)) = prefix.rsplit_once('/') {
            confirm_inability_to_guarantee_prefix(prefix, Some(directory))?;
            Some(directory)
        } else {
            confirm_inability_to_guarantee_prefix(prefix, None)?;
            None
        }
    };

    let mut access_key: String = String::new();
    print!("Enter access key for storage account {}: ", storage_account);
    std::io::Write::flush(&mut std::io::stdout().lock()).expect("stdout");
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut access_key)
        .expect("stdin");
    access_key = String::from(access_key.trim());

    let permissions: azure_storage::shared_access_signature::service_sas::BlobSasPermissions =
        azure_storage::shared_access_signature::service_sas::BlobSasPermissions {
            // This really should be just nothing except "create: true" and
            // nothing else. It turns out that "create: true" works for
            // the Put Blob API call, but not for the Put Block List API call.
            // So we have to give full write access, which is more than what we
            // would like, but we don't have a choice here.
            write: true,
            ..Default::default()
        };
    let expiry: std::time::SystemTime = std::time::SystemTime::now()
        + std::time::Duration::from_secs(86400 * 365 * 100);
    let (resource, canonicalized_resource) = if let Some(directory) = directory
    {
        (
            azure_storage::shared_access_signature::service_sas::BlobSignedResource::Directory,
            format!("/blob/{}/{}/{}", storage_account, container, directory),
        )
    } else {
        (
            azure_storage::shared_access_signature::service_sas::BlobSignedResource::Container,
            format!("/blob/{}/{}", storage_account, container),
        )
    };
    let secret: azure_core::auth::Secret =
        azure_core::auth::Secret::new(access_key);
    let sas_key: azure_storage::shared_access_signature::service_sas::SasKey =
        azure_storage::shared_access_signature::service_sas::SasKey::Key(
            secret,
        );
    let mut sas_generator: azure_storage::shared_access_signature::service_sas::BlobSharedAccessSignature =
        azure_storage::shared_access_signature::service_sas::BlobSharedAccessSignature::new(
            sas_key,
            canonicalized_resource,
            permissions,
            expiry.into(),
            resource,
        );
    if let Some(directory) = directory {
        let num_slashes: usize =
            directory.bytes().filter(|c: &u8| *c == b'/').count();
        sas_generator = sas_generator.signed_directory_depth(num_slashes + 1);
    }

    let sas: String = azure_storage::prelude::SasToken::token(&sas_generator)?;

    println!();
    println!("Generated SAS: {}", sas);

    let mut example_confg_file_syntax: scan2blob::util::BlobStorageSpec =
        scan2blob::util::BlobStorageSpec {
            storage_account: storage_account.clone(),
            container: container.clone(),
            sas: scan2blob::util::Sas::Literal(sas),
            prefix: prefix.clone(),
        };

    println!();
    println!(
        "This can be represented in the server's config file as a plain string, like so:"
    );
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &example_confg_file_syntax,
    )?;
    println!();

    example_confg_file_syntax.sas =
        scan2blob::util::Sas::EnvironmentVariable {
            env: "NAME_OF_ENV_VAR".into(),
        };

    println!();
    println!(
        "Or else you can set an environment variable to contain the real value, and"
    );
    println!(
        "then refer to the environment variable in the config file like so:"
    );
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &example_confg_file_syntax,
    )?;
    println!();

    Ok(())
}
