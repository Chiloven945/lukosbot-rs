use std::collections::HashSet;
use std::fmt::Write;

use crate::args::ArgType;
use crate::registry::CommandRegistry;
use crate::spec::{CommandDefinition, CommandNode, InputSpec, SyntaxLine, SyntaxPart};

/// Renders the list of visible commands.
pub fn render_list(registry: &CommandRegistry, prefix: &str) -> String {
    let mut output = String::from("可用命令：\n");

    for command in registry.visible_commands() {
        let _ = write!(output, "{prefix}{}", command.name);
        if !command.aliases.is_empty() {
            let _ = write!(output, " ({})", command.aliases.join(", "));
        }
        let _ = writeln!(output, " - {}", command.description);
    }

    let _ = writeln!(
        output,
        "\n发送 {prefix}help <command> 查看某个命令的详细用法。"
    );
    output.trim_end().to_owned()
}

/// Renders the detailed usage for one command.
pub fn render_command(definition: &CommandDefinition, prefix: &str) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "命令：{prefix}{}", definition.name);
    let _ = writeln!(output, "说明：{}", definition.description);

    if !definition.aliases.is_empty() {
        let _ = writeln!(output, "别名：{}", definition.aliases.join(", "));
    }

    let syntaxes = collect_syntaxes(definition, prefix);
    if !syntaxes.is_empty() {
        let _ = writeln!(output, "\n用法：");
        for (invocation, description) in syntaxes {
            let _ = write!(output, "  {invocation}");
            if let Some(description) = description.filter(|description| !description.is_empty()) {
                let _ = write!(output, " — {description}");
            }
            output.push('\n');
        }
    }

    let documentation = parameter_docs(&definition.root);
    if !documentation.is_empty() {
        let _ = writeln!(output, "\n参数与选项：");
        for line in documentation {
            let _ = writeln!(output, "  {line}");
        }
    }

    if !definition.usage.examples.is_empty() {
        let _ = writeln!(output, "\n示例：");
        for example in &definition.usage.examples {
            let _ = writeln!(output, "  {}", normalize_example(example, prefix));
        }
    }

    if !definition.usage.notes.is_empty() {
        let _ = writeln!(output, "\n提示：");
        for note in &definition.usage.notes {
            let _ = writeln!(output, "  {note}");
        }
    }

    let children = child_docs(&definition.root, prefix);
    if !children.is_empty() {
        let _ = writeln!(output, "\n子命令：");
        for line in children {
            let _ = writeln!(output, "  {line}");
        }
    }

    output.trim_end().to_owned()
}

fn collect_syntaxes(definition: &CommandDefinition, prefix: &str) -> Vec<(String, Option<String>)> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    collect_node_syntax(
        &definition.root,
        &[definition.name.as_str()],
        prefix,
        &definition.usage.syntaxes,
        &mut output,
        &mut seen,
    );
    output
}

fn collect_node_syntax(
    node: &CommandNode,
    path: &[&str],
    prefix: &str,
    explicit: &[SyntaxLine],
    output: &mut Vec<(String, Option<String>)>,
    seen: &mut HashSet<String>,
) {
    if explicit.is_empty() {
        if node.executor.is_some() {
            let invocation = render_auto_invocation(path, &node.input, prefix);
            if seen.insert(invocation.clone()) {
                output.push((invocation, None));
            }
        }
    } else {
        for syntax in explicit {
            let invocation = format!(
                "{}{}",
                prefix,
                join_path_and_tail(path, &render_syntax_parts(&syntax.parts))
            );
            if seen.insert(invocation.clone()) {
                output.push((invocation, Some(syntax.description.clone())));
            }
        }
    }

    for child in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(child.name.as_str());
        collect_node_syntax(
            child,
            &child_path,
            prefix,
            &child.usage.syntaxes,
            output,
            seen,
        );
    }
}

fn render_auto_invocation(path: &[&str], input: &InputSpec, prefix: &str) -> String {
    let tail = match input {
        InputSpec::NoArgs => String::new(),
        InputSpec::Raw { name, required } => {
            if *required {
                format!("<{name}>")
            } else {
                format!("[{name}]")
            }
        }
        InputSpec::Args(args) => {
            let mut parts = Vec::new();
            for positional in &args.positionals {
                let name = if positional.greedy {
                    format!("<{}>...", positional.name)
                } else if positional.required {
                    format!("<{}>", positional.name)
                } else {
                    format!("[{}]", positional.name)
                };
                parts.push(name);
            }
            for option in &args.options {
                let name = option
                    .names
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("--{}", option.canonical_name));
                let value = match option.arg_type {
                    ArgType::Bool => name,
                    _ => format!("{name}=<{}>", option.canonical_name),
                };
                if option.required {
                    parts.push(value);
                } else {
                    parts.push(format!("[{value}]"));
                }
            }
            parts.join(" ")
        }
    };

    format!("{}{}", prefix, join_path_and_tail(path, &tail))
}

fn join_path_and_tail(path: &[&str], tail: &str) -> String {
    let path = path.join(" ");
    if tail.is_empty() {
        path
    } else {
        format!("{path} {tail}")
    }
}

fn render_syntax_parts(parts: &[SyntaxPart]) -> String {
    parts
        .iter()
        .map(render_syntax_part)
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_syntax_part(part: &SyntaxPart) -> String {
    match part {
        SyntaxPart::Literal(value) => value.clone(),
        SyntaxPart::Argument { name, optional } => {
            if *optional {
                format!("[{name}]")
            } else {
                format!("<{name}>")
            }
        }
        SyntaxPart::Option {
            names,
            value_name,
            optional,
        } => {
            let name = names
                .first()
                .cloned()
                .unwrap_or_else(|| "--option".to_owned());
            let rendered = match value_name {
                Some(value_name) => format!("{name}=<{value_name}>"),
                None => name,
            };
            if *optional {
                format!("[{rendered}]")
            } else {
                rendered
            }
        }
        SyntaxPart::OneOf(parts) => format!(
            "({})",
            parts
                .iter()
                .map(render_syntax_part)
                .collect::<Vec<_>>()
                .join("|")
        ),
    }
}

fn parameter_docs(node: &CommandNode) -> Vec<String> {
    let InputSpec::Args(args) = &node.input else {
        return Vec::new();
    };

    let mut output = Vec::new();
    for positional in &args.positionals {
        let mut line = positional.name.clone();
        if !positional.description.is_empty() {
            let _ = write!(line, " — {}", positional.description);
        }
        if let Some(default) = &positional.default {
            let _ = write!(line, "（默认 {default}）");
        }
        output.push(line);
    }

    for option in &args.options {
        let names = option.names.join(", ");
        let mut line = match option.arg_type {
            ArgType::Bool => names,
            _ => format!("{names} <{}>", option.canonical_name),
        };
        if !option.description.is_empty() {
            let _ = write!(line, " — {}", option.description);
        }
        if let Some(default) = &option.default {
            let _ = write!(line, "（默认 {default}）");
        }
        output.push(line);
    }

    output
}

fn child_docs(node: &CommandNode, prefix: &str) -> Vec<String> {
    let mut output = Vec::new();
    for child in &node.children {
        output.push(format!("{prefix}{} — {}", child.name, child.description));
    }
    output
}

fn normalize_example(example: &str, prefix: &str) -> String {
    let example = example.trim();
    if example.starts_with(prefix) {
        example.to_owned()
    } else {
        format!("{prefix}{example}")
    }
}
