use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const MAX_TEMPLATE_BYTES: usize = 1024 * 1024;
pub const MAX_TEMPLATE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TEMPLATE_VARIABLES: usize = 64;
pub const MAX_TEMPLATE_VALUE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableEncoding {
    XmlText,
    JsonString,
    UrlComponent,
    Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateVariableSpec {
    pub name: String,
    pub encoding: VariableEncoding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateManifest {
    pub relative_path: String,
    pub variables: Vec<TemplateVariableSpec>,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Variable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledTemplate {
    segments: Vec<Segment>,
    variables: BTreeMap<String, VariableEncoding>,
    max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TemplateError {}

impl CompiledTemplate {
    pub fn compile(source: &[u8], manifest: &TemplateManifest) -> Result<Self, TemplateError> {
        validate_manifest(manifest)?;
        if source.is_empty() || source.len() > MAX_TEMPLATE_BYTES {
            return Err(error(
                "device_simulator.template.size_invalid",
                format!("template size must be between 1 and {MAX_TEMPLATE_BYTES} bytes"),
            ));
        }
        let source = std::str::from_utf8(source).map_err(|source| {
            error(
                "device_simulator.template.encoding_invalid",
                format!("template is not valid UTF-8: {source}"),
            )
        })?;
        let variables = manifest
            .variables
            .iter()
            .map(|item| (item.name.clone(), item.encoding))
            .collect::<BTreeMap<_, _>>();
        let mut referenced = BTreeSet::new();
        let mut segments = Vec::new();
        let mut cursor = 0;
        while let Some(relative_start) = source[cursor..].find("{{") {
            let start = cursor + relative_start;
            if start > cursor {
                if source[cursor..start].contains("}}") {
                    return Err(error(
                        "device_simulator.template.placeholder_malformed",
                        "template contains an unmatched placeholder terminator",
                    ));
                }
                segments.push(Segment::Literal(source[cursor..start].to_owned()));
            }
            let value_start = start + 2;
            let relative_end = source[value_start..].find("}}").ok_or_else(|| {
                error(
                    "device_simulator.template.placeholder_malformed",
                    "template contains an unterminated placeholder",
                )
            })?;
            let end = value_start + relative_end;
            let name = &source[value_start..end];
            if !is_safe_variable_name(name) {
                return Err(error(
                    "device_simulator.template.variable_name_invalid",
                    format!("template variable '{name}' is not a safe dotted identifier"),
                ));
            }
            if !variables.contains_key(name) {
                return Err(error(
                    "device_simulator.template.variable_undeclared",
                    format!("template variable '{name}' is not declared by its manifest"),
                ));
            }
            referenced.insert(name.to_owned());
            segments.push(Segment::Variable(name.to_owned()));
            cursor = end + 2;
        }
        if source[cursor..].contains("}}") {
            return Err(error(
                "device_simulator.template.placeholder_malformed",
                "template contains an unmatched placeholder terminator",
            ));
        }
        if cursor < source.len() {
            segments.push(Segment::Literal(source[cursor..].to_owned()));
        }
        let declared = variables.keys().cloned().collect::<BTreeSet<_>>();
        if referenced != declared {
            let unused = declared
                .difference(&referenced)
                .cloned()
                .collect::<Vec<_>>();
            return Err(error(
                "device_simulator.template.variable_unused",
                format!("manifest declares unused variables: {}", unused.join(", ")),
            ));
        }
        Ok(Self {
            segments,
            variables,
            max_output_bytes: manifest.max_output_bytes,
        })
    }

    pub fn compile_from_pack(
        pack_root: &Path,
        manifest: &TemplateManifest,
    ) -> Result<Self, TemplateError> {
        let relative = normalize_template_path(&manifest.relative_path)?;
        let path = pack_root.join(relative);
        reject_symlink_components(pack_root, &path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|source| {
            error(
                "device_simulator.template.read_failed",
                format!("failed to inspect template '{}': {source}", path.display()),
            )
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(error(
                "device_simulator.template.file_invalid",
                "template must be a non-empty regular file",
            ));
        }
        if metadata.len() > MAX_TEMPLATE_BYTES as u64 {
            return Err(error(
                "device_simulator.template.size_invalid",
                "template is larger than the supported limit",
            ));
        }
        let source = fs::read(&path).map_err(|source| {
            error(
                "device_simulator.template.read_failed",
                format!("failed to read template '{}': {source}", path.display()),
            )
        })?;
        Self::compile(&source, manifest)
    }

    pub fn render(&self, values: &BTreeMap<String, String>) -> Result<Vec<u8>, TemplateError> {
        for name in self.variables.keys() {
            if !values.contains_key(name) {
                return Err(error(
                    "device_simulator.template.variable_missing",
                    format!("render value for '{name}' is missing"),
                ));
            }
        }
        if let Some(extra) = values
            .keys()
            .find(|name| !self.variables.contains_key(*name))
        {
            return Err(error(
                "device_simulator.template.variable_unexpected",
                format!("render value '{extra}' is not declared by the template"),
            ));
        }

        let mut output = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(value) => {
                    append_bounded(&mut output, value, self.max_output_bytes)?
                }
                Segment::Variable(name) => {
                    let value = &values[name];
                    if value.len() > MAX_TEMPLATE_VALUE_BYTES {
                        return Err(error(
                            "device_simulator.template.value_too_large",
                            format!("render value '{name}' exceeds the supported limit"),
                        ));
                    }
                    let encoded = encode_value(value, self.variables[name])?;
                    append_bounded(&mut output, &encoded, self.max_output_bytes)?;
                }
            }
        }
        Ok(output.into_bytes())
    }
}

pub fn normalize_template_path(value: &str) -> Result<PathBuf, TemplateError> {
    if value.is_empty()
        || value.len() > 240
        || value.contains('\\')
        || value.starts_with('/')
        || value.contains('\0')
    {
        return Err(error(
            "device_simulator.template.path_invalid",
            "template path must be a short relative forward-slash path",
        ));
    }
    let path = Path::new(value);
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 2 || components.first() != Some(&Component::Normal("templates".as_ref()))
    {
        return Err(error(
            "device_simulator.template.path_invalid",
            "template must be stored below templates/",
        ));
    }
    for component in components {
        let Component::Normal(segment) = component else {
            return Err(error(
                "device_simulator.template.path_invalid",
                "template path contains a non-normal component",
            ));
        };
        let Some(segment) = segment.to_str() else {
            return Err(error(
                "device_simulator.template.path_invalid",
                "template path is not UTF-8",
            ));
        };
        if segment.is_empty()
            || !segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(error(
                "device_simulator.template.path_invalid",
                format!("template path segment '{segment}' is not allowed"),
            ));
        }
    }
    Ok(path.to_path_buf())
}

fn validate_manifest(manifest: &TemplateManifest) -> Result<(), TemplateError> {
    normalize_template_path(&manifest.relative_path)?;
    if manifest.max_output_bytes == 0 || manifest.max_output_bytes > MAX_TEMPLATE_OUTPUT_BYTES {
        return Err(error(
            "device_simulator.template.output_limit_invalid",
            format!("template output limit must be between 1 and {MAX_TEMPLATE_OUTPUT_BYTES}"),
        ));
    }
    if manifest.variables.len() > MAX_TEMPLATE_VARIABLES {
        return Err(error(
            "device_simulator.template.variable_count_invalid",
            format!("template declares more than {MAX_TEMPLATE_VARIABLES} variables"),
        ));
    }
    let mut names = BTreeSet::new();
    for variable in &manifest.variables {
        if !is_safe_variable_name(&variable.name) {
            return Err(error(
                "device_simulator.template.variable_name_invalid",
                format!(
                    "variable '{}' is not a safe dotted identifier",
                    variable.name
                ),
            ));
        }
        if !names.insert(variable.name.as_str()) {
            return Err(error(
                "device_simulator.template.variable_duplicate",
                format!("variable '{}' is declared more than once", variable.name),
            ));
        }
    }
    Ok(())
}

fn reject_symlink_components(root: &Path, path: &Path) -> Result<(), TemplateError> {
    if fs::symlink_metadata(root)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(error(
            "device_simulator.template.symlink_rejected",
            "template pack root must not be a symlink",
        ));
    }
    let relative = path.strip_prefix(root).map_err(|_| {
        error(
            "device_simulator.template.path_invalid",
            "template path escaped its pack root",
        )
    })?;
    let mut candidate = root.to_path_buf();
    for component in relative.components() {
        candidate.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&candidate) {
            if metadata.file_type().is_symlink() {
                return Err(error(
                    "device_simulator.template.symlink_rejected",
                    format!("template path '{}' contains a symlink", candidate.display()),
                ));
            }
        }
    }
    Ok(())
}

fn is_safe_variable_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn encode_value(value: &str, encoding: VariableEncoding) -> Result<String, TemplateError> {
    match encoding {
        VariableEncoding::XmlText => {
            if value
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
            {
                return Err(error(
                    "device_simulator.template.xml_value_invalid",
                    "XML template value contains a forbidden control character",
                ));
            }
            Ok(value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;"))
        }
        VariableEncoding::JsonString => {
            let encoded = serde_json::to_string(value).map_err(|source| {
                error(
                    "device_simulator.template.value_encoding_failed",
                    format!("failed to encode JSON string: {source}"),
                )
            })?;
            Ok(encoded[1..encoded.len() - 1].to_owned())
        }
        VariableEncoding::UrlComponent => {
            let mut output = String::new();
            for byte in value.as_bytes() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    output.push(*byte as char);
                } else {
                    output.push_str(&format!("%{byte:02X}"));
                }
            }
            Ok(output)
        }
        VariableEncoding::Decimal => {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(error(
                    "device_simulator.template.decimal_invalid",
                    "decimal template values may contain only ASCII digits",
                ));
            }
            Ok(value.to_owned())
        }
    }
}

fn append_bounded(output: &mut String, value: &str, limit: usize) -> Result<(), TemplateError> {
    let next_len = output.len().checked_add(value.len()).ok_or_else(|| {
        error(
            "device_simulator.template.output_too_large",
            "rendered template length overflowed",
        )
    })?;
    if next_len > limit {
        return Err(error(
            "device_simulator.template.output_too_large",
            format!("rendered template exceeds its {limit}-byte limit"),
        ));
    }
    output.push_str(value);
    Ok(())
}

fn error(code: &'static str, message: impl Into<String>) -> TemplateError {
    TemplateError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(variables: Vec<TemplateVariableSpec>) -> TemplateManifest {
        TemplateManifest {
            relative_path: "templates/common/search.xml".into(),
            variables,
            max_output_bytes: 1024,
        }
    }

    #[test]
    fn compiles_declared_variables_and_encodes_by_context() {
        let template = CompiledTemplate::compile(
            br#"<d ip="{{device.ip}}">{{device.model}}</d><n>{{device.count}}</n>"#,
            &manifest(vec![
                TemplateVariableSpec {
                    name: "device.ip".into(),
                    encoding: VariableEncoding::XmlText,
                },
                TemplateVariableSpec {
                    name: "device.model".into(),
                    encoding: VariableEncoding::XmlText,
                },
                TemplateVariableSpec {
                    name: "device.count".into(),
                    encoding: VariableEncoding::Decimal,
                },
            ]),
        )
        .unwrap();
        let rendered = template
            .render(&BTreeMap::from([
                ("device.ip".into(), "192.0.2.2".into()),
                ("device.model".into(), "A&B<1>".into()),
                ("device.count".into(), "8".into()),
            ]))
            .unwrap();
        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            r#"<d ip="192.0.2.2">A&amp;B&lt;1&gt;</d><n>8</n>"#
        );
    }

    #[test]
    fn rejects_undeclared_unused_missing_and_unexpected_variables() {
        assert_eq!(
            CompiledTemplate::compile(b"{{device.unknown}}", &manifest(vec![]),)
                .unwrap_err()
                .code,
            "device_simulator.template.variable_undeclared"
        );
        assert_eq!(
            CompiledTemplate::compile(
                b"static",
                &manifest(vec![TemplateVariableSpec {
                    name: "device.ip".into(),
                    encoding: VariableEncoding::XmlText,
                }]),
            )
            .unwrap_err()
            .code,
            "device_simulator.template.variable_unused"
        );
        let template = CompiledTemplate::compile(
            b"{{device.ip}}",
            &manifest(vec![TemplateVariableSpec {
                name: "device.ip".into(),
                encoding: VariableEncoding::XmlText,
            }]),
        )
        .unwrap();
        assert_eq!(
            template.render(&BTreeMap::new()).unwrap_err().code,
            "device_simulator.template.variable_missing"
        );
        assert_eq!(
            template
                .render(&BTreeMap::from([
                    ("device.ip".into(), "192.0.2.2".into()),
                    ("secret".into(), "do-not-render".into()),
                ]))
                .unwrap_err()
                .code,
            "device_simulator.template.variable_unexpected"
        );
    }

    #[test]
    fn rejects_path_traversal_invalid_utf8_and_output_overflow() {
        assert_eq!(
            normalize_template_path("templates/../secret.xml")
                .unwrap_err()
                .code,
            "device_simulator.template.path_invalid"
        );
        assert_eq!(
            CompiledTemplate::compile(&[0xff], &manifest(vec![]))
                .unwrap_err()
                .code,
            "device_simulator.template.encoding_invalid"
        );
        let mut limited = manifest(vec![TemplateVariableSpec {
            name: "device.ip".into(),
            encoding: VariableEncoding::XmlText,
        }]);
        limited.max_output_bytes = 3;
        let template = CompiledTemplate::compile(b"{{device.ip}}", &limited).unwrap();
        assert_eq!(
            template
                .render(&BTreeMap::from([("device.ip".into(), "long".into())]))
                .unwrap_err()
                .code,
            "device_simulator.template.output_too_large"
        );
    }

    #[test]
    fn context_encoders_reject_unsafe_decimal_and_escape_json() {
        assert_eq!(
            encode_value("12x", VariableEncoding::Decimal)
                .unwrap_err()
                .code,
            "device_simulator.template.decimal_invalid"
        );
        assert_eq!(
            encode_value("a\"\n", VariableEncoding::JsonString).unwrap(),
            "a\\\"\\n"
        );
        assert_eq!(
            encode_value("a/b c", VariableEncoding::UrlComponent).unwrap(),
            "a%2Fb%20c"
        );
        assert_eq!(
            encode_value("bad\u{1}", VariableEncoding::XmlText)
                .unwrap_err()
                .code,
            "device_simulator.template.xml_value_invalid"
        );
        assert_eq!(
            CompiledTemplate::compile(
                b"stray }} {{device.ip}}",
                &manifest(vec![TemplateVariableSpec {
                    name: "device.ip".into(),
                    encoding: VariableEncoding::XmlText,
                },])
            )
            .unwrap_err()
            .code,
            "device_simulator.template.placeholder_malformed"
        );
    }
}
