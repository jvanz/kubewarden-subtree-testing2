use std::{
    collections::HashMap,
    convert::TryFrom,
    io::{self},
    str::FromStr,
};

use anyhow::{anyhow, Result};
use is_terminal::IsTerminal;
use policy_evaluator::{
    constants::*,
    policy_evaluator::PolicyExecutionMode,
    policy_fetcher::{
        oci_client::{
            manifest::{OciImageManifest, OciManifest},
            secrets::RegistryAuth,
        },
        registry::Registry,
        sigstore::{
            cosign::{ClientBuilder, CosignCapabilities},
            registry::{oci_reference::OciReference, Auth, ClientConfig},
        },
        sources::Sources,
    },
    policy_metadata::Metadata,
};
use prettytable::{format::FormatBuilder, row, Table};
use termimad::{terminal_size, FmtText, MadSkin};

pub(crate) async fn inspect(
    uri_or_sha_prefix: &str,
    output: OutputType,
    sources: Option<Sources>,
    no_color: bool,
    no_signatures: bool,
) -> Result<()> {
    let uri = crate::utils::map_path_to_uri(uri_or_sha_prefix)?;
    let wasm_path = crate::utils::wasm_path(&uri)?;
    let metadata_printer = MetadataPrinter::from(&output);

    let metadata = Metadata::from_path(&wasm_path)
        .map_err(|e| anyhow!("Error parsing policy metadata: {}", e))?;

    match metadata {
        Some(metadata) => metadata_printer.print(&metadata, no_color)?,
        None => return Err(anyhow!(
            "No Kubewarden metadata found inside of '{}'.\nPolicies can be annotated with the `kwctl annotate` command.",
            uri
        )),
    };

    if no_signatures {
        return Ok(());
    }

    let signatures = fetch_signatures_manifest(&uri, sources).await;
    match signatures {
        Ok(signatures) => {
            if let Some(signatures) = signatures {
                let sigstore_printer = SignaturesPrinter::from(&output);
                sigstore_printer.print(&signatures);
            }
        }
        Err(error) => {
            println!();
            if error
                .to_string()
                .as_str()
                .starts_with("OCI API error: manifest unknown on")
            {
                println!("No sigstore signatures found");
            } else {
                println!("Cannot determine if the policy has been signed. There was an error while attempting to fetch its signatures from the remote registry: {error} ")
            }
        }
    }

    Ok(())
}

pub(crate) enum OutputType {
    Yaml,
    Pretty,
}

impl TryFrom<Option<&str>> for OutputType {
    type Error = anyhow::Error;

    fn try_from(value: Option<&str>) -> Result<Self, Self::Error> {
        match value {
            Some("yaml") => Ok(Self::Yaml),
            None => Ok(Self::Pretty),
            Some(unknown) => Err(anyhow!("Invalid output format '{}'", unknown)),
        }
    }
}

enum MetadataPrinter {
    Yaml,
    Pretty,
}

impl From<&OutputType> for MetadataPrinter {
    fn from(output_type: &OutputType) -> Self {
        match output_type {
            OutputType::Yaml => Self::Yaml,
            OutputType::Pretty => Self::Pretty,
        }
    }
}

impl MetadataPrinter {
    fn print(&self, metadata: &Metadata, no_color: bool) -> Result<()> {
        match self {
            MetadataPrinter::Yaml => {
                let metadata_yaml = serde_yaml::to_string(metadata)?;
                print!("{metadata_yaml}");
                Ok(())
            }
            MetadataPrinter::Pretty => {
                self.print_metadata_generic_info(metadata)?;
                println!();
                self.print_metadata_rules(metadata, no_color)?;
                println!();
                if !metadata.context_aware_resources.is_empty() {
                    self.print_metadata_context_aware_resources(metadata, no_color)?;
                    println!();
                }
                self.print_metadata_usage(metadata, no_color);
                Ok(())
            }
        }
    }

    fn annotation_to_row_key(&self, text: &str) -> String {
        let mut out = String::from(text);
        out.push(':');
        String::from(out.trim_start_matches("io.kubewarden.policy."))
    }

    fn print_metadata_generic_info(&self, metadata: &Metadata) -> Result<()> {
        let protocol_version = metadata
            .protocol_version
            .clone()
            .ok_or_else(|| anyhow!("Invalid policy: protocol_version not defined"))?;

        let pretty_annotations = [
            KUBEWARDEN_ANNOTATION_POLICY_TITLE,
            KUBEWARDEN_ANNOTATION_POLICY_DESCRIPTION,
            KUBEWARDEN_ANNOTATION_POLICY_AUTHOR,
            KUBEWARDEN_ANNOTATION_POLICY_URL,
            KUBEWARDEN_ANNOTATION_POLICY_SOURCE,
            KUBEWARDEN_ANNOTATION_POLICY_LICENSE,
        ];
        let mut annotations = metadata.annotations.clone().unwrap_or_default();

        let mut table = Table::new();
        table.set_format(FormatBuilder::new().padding(0, 1).build());

        table.add_row(row![Fmbl -> "Details"]);
        for annotation in pretty_annotations.iter() {
            if let Some(value) = annotations.get(&String::from(*annotation)) {
                table.add_row(row![Fgbl -> self.annotation_to_row_key(annotation), d -> value]);
                annotations.remove(&String::from(*annotation));
            }
        }
        table.add_row(row![Fgbl -> "mutating:", metadata.mutating]);
        table.add_row(row![Fgbl -> "background audit support:", metadata.background_audit]);
        table.add_row(row![Fgbl -> "context aware:", !metadata.context_aware_resources.is_empty()]);
        table.add_row(row![Fgbl -> "policy type:", metadata.policy_type]);
        table.add_row(row![Fgbl -> "execution mode:", metadata.execution_mode]);
        if metadata.execution_mode == PolicyExecutionMode::KubewardenWapc {
            table.add_row(row![Fgbl -> "protocol version:", protocol_version]);
        }
        if let Some(minimum_kubewarden_version) = &metadata.minimum_kubewarden_version {
            table.add_row(row![Fgbl -> "minimum kubewarden version:", minimum_kubewarden_version]);
        }

        let _usage = annotations.remove(KUBEWARDEN_ANNOTATION_POLICY_USAGE);
        if !annotations.is_empty() {
            table.add_row(row![]);
            table.add_row(row![Fmbl -> "Annotations"]);
            for (annotation, value) in annotations.iter() {
                table.add_row(row![Fgbl -> annotation, d -> value]);
            }
        }
        table.printstd();
        Ok(())
    }

    fn print_metadata_rules(&self, metadata: &Metadata, no_color: bool) -> Result<()> {
        let rules_yaml = serde_yaml::to_string(&metadata.rules)?;

        // Quick hack to print a colorized "Rules" section, with the same
        // style as the other sections we print
        let mut table = Table::new();
        table.set_format(FormatBuilder::new().padding(0, 1).build());
        table.add_row(row![Fmbl -> "Rules"]);
        table.printstd();

        let text = format!("```yaml\n{rules_yaml}```");
        self.render_markdown(&text, no_color);
        Ok(())
    }

    fn print_metadata_context_aware_resources(
        &self,
        metadata: &Metadata,
        no_color: bool,
    ) -> Result<()> {
        let resources_yaml = serde_yaml::to_string(&metadata.context_aware_resources)?;

        // Quick hack to print a colorized "Context Aware" section, with the same
        // style as the other sections we print
        let mut table = Table::new();
        table.set_format(FormatBuilder::new().padding(0, 1).build());
        table.add_row(row![Fmbl -> "Context Aware"]);
        table.printstd();

        println!(
            "The policy requires access to the following Kubernetes resources at evaluation time:"
        );

        let text = format!("```yaml\n{resources_yaml}```");
        self.render_markdown(&text, no_color);
        println!("To avoid abuses, review carefully what the policy requires access to.");

        Ok(())
    }

    fn print_metadata_usage(&self, metadata: &Metadata, no_color: bool) {
        let usage = match metadata.annotations.clone() {
            None => None,
            Some(annotations) => annotations
                .get(KUBEWARDEN_ANNOTATION_POLICY_USAGE)
                .map(String::from),
        };

        if usage.is_none() {
            return;
        }

        // Quick hack to print a colorized "Rules" section, with the same
        // style as the other sections we print
        let mut table = Table::new();
        table.set_format(FormatBuilder::new().padding(0, 1).build());
        table.add_row(row![Fmbl -> "Usage"]);
        table.printstd();

        let fenced_usage = format!("---\n{}\n---", usage.unwrap());
        self.render_markdown(&fenced_usage, no_color);
    }

    fn render_markdown(&self, text: &str, no_color: bool) {
        let mut skin: MadSkin = if no_color || !io::stdout().is_terminal() {
            MadSkin::no_style()
        } else {
            MadSkin::default()
        };
        skin.headers[0].align = termimad::Alignment::Left;

        let (mut width, _) = terminal_size();
        if width > 120 {
            // limit width to print nicer rulers
            width = 120;
        }
        let fmt_text = FmtText::from_text(&skin, text.into(), Some(width as usize));
        print!("{}", fmt_text);
    }
}

enum SignaturesPrinter {
    Yaml,
    Pretty,
}

impl From<&OutputType> for SignaturesPrinter {
    fn from(output_type: &OutputType) -> Self {
        match output_type {
            OutputType::Yaml => Self::Yaml,
            OutputType::Pretty => Self::Pretty,
        }
    }
}

impl SignaturesPrinter {
    fn print(&self, signatures: &OciImageManifest) {
        match self {
            SignaturesPrinter::Yaml => {
                let mut doc_entry: HashMap<String, &OciImageManifest> = HashMap::new();
                doc_entry.insert("signatures".to_string(), signatures);

                let signatures_yaml = serde_yaml::to_string(&doc_entry);
                if let Ok(signatures_yaml) = signatures_yaml {
                    print!("{signatures_yaml}")
                }
            }
            SignaturesPrinter::Pretty => {
                println!();
                println!("Sigstore signatures");
                println!();

                for layer in &signatures.layers {
                    let mut table = Table::new();
                    table.set_format(FormatBuilder::new().padding(0, 1).build());
                    table.add_row(row![Fmbl -> "Digest: ", layer.digest]);
                    table.add_row(row![Fmbl -> "Media type: ", layer.media_type]);
                    table.add_row(row![Fmbl -> "Size: ", layer.size]);
                    if let Some(annotations) = &layer.annotations {
                        table.add_row(row![Fmbl -> "Annotations"]);
                        for annotation in annotations.iter() {
                            table.add_row(row![Fgbl -> annotation.0, annotation.1]);
                        }
                    }
                    table.printstd();
                    println!();
                }
            }
        }
    }
}

async fn fetch_signatures_manifest(
    uri: &str,
    sources: Option<Sources>,
) -> Result<Option<OciImageManifest>> {
    let registry = Registry::new();
    let client_config: ClientConfig = sources.clone().unwrap_or_default().into();
    let mut client = ClientBuilder::default()
        .with_oci_client_config(client_config)
        .build()?;
    let image_name = uri
        .strip_prefix("registry://")
        .ok_or_else(|| anyhow!("invalid uri"))?;
    let image_ref = OciReference::from_str(image_name)?;
    let auth = match Registry::auth(image_name) {
        RegistryAuth::Anonymous => Auth::Anonymous,
        RegistryAuth::Basic(username, password) => Auth::Basic(username, password),
        RegistryAuth::Bearer(token) => Auth::Bearer(token),
    };

    let (cosign_signature_image, _source_image_digest) =
        client.triangulate(&image_ref, &auth).await?;

    let manifest = registry
        .manifest(&cosign_signature_image.whole(), sources.as_ref())
        .await?;

    match manifest {
        OciManifest::Image(img) => Ok(Some(img)),
        _ => Ok(None),
    }
}
