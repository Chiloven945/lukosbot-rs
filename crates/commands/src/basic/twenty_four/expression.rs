use thiserror::Error;

pub const TARGET: f64 = 24.0;
pub const EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq)]
pub struct EvalResult {
    pub value: f64,
    pub numbers: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ExpressionError {
    message: String,
}

impl ExpressionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Evaluates a 24-point expression without invoking a shell or a general-purpose evaluator.
pub fn evaluate(input: &str) -> Result<EvalResult, ExpressionError> {
    let normalized: String = input
        .chars()
        .filter(|character| *character != '\\' && !character.is_whitespace())
        .collect();
    if normalized.is_empty() {
        return Err(ExpressionError::new("表达式不能为空"));
    }

    let tokens = tokenize(&normalized)?;
    let mut values: Vec<f64> = Vec::new();
    let mut operators: Vec<char> = Vec::new();
    let mut numbers = Vec::new();

    for token in tokens {
        match token {
            Token::Number(number) => {
                numbers.push(number);
                values.push(f64::from(number));
            }
            Token::LeftParen => operators.push('('),
            Token::RightParen => {
                while let Some(operator) = operators.last().copied() {
                    if operator == '(' {
                        break;
                    }
                    apply_operator(&mut values, operator)?;
                    operators.pop();
                }
                if operators.pop() != Some('(') {
                    return Err(ExpressionError::new("括号不匹配"));
                }
            }
            Token::Operator(operator) => {
                while let Some(previous) = operators.last().copied() {
                    if previous == '(' || precedence(previous) < precedence(operator) {
                        break;
                    }
                    apply_operator(&mut values, previous)?;
                    operators.pop();
                }
                operators.push(operator);
            }
        }
    }

    while let Some(operator) = operators.pop() {
        if operator == '(' || operator == ')' {
            return Err(ExpressionError::new("括号不匹配"));
        }
        apply_operator(&mut values, operator)?;
    }

    if values.len() != 1 {
        return Err(ExpressionError::new("表达式不完整"));
    }

    let value = values.pop().expect("one value remains");
    Ok(EvalResult { value, numbers })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Number(i32),
    Operator(char),
    LeftParen,
    RightParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ExpressionError> {
    let mut tokens = Vec::new();
    let mut characters = input.char_indices().peekable();

    while let Some((_, character)) = characters.next() {
        match character {
            '0'..='9' => {
                let mut number = String::from(character);
                while let Some((_, next)) = characters.peek().copied() {
                    if next.is_ascii_digit() {
                        number.push(next);
                        characters.next();
                    } else {
                        break;
                    }
                }
                let number = number
                    .parse::<i32>()
                    .map_err(|_| ExpressionError::new(format!("数字过大：{number}")))?;
                tokens.push(Token::Number(number));
            }
            '+' | '-' | '*' | '/' => tokens.push(Token::Operator(character)),
            '(' => tokens.push(Token::LeftParen),
            ')' => tokens.push(Token::RightParen),
            _ => {
                return Err(ExpressionError::new(format!(
                    "包含不支持的字符：{character}"
                )));
            }
        }
    }

    Ok(tokens)
}

fn precedence(operator: char) -> u8 {
    match operator {
        '+' | '-' => 1,
        '*' | '/' => 2,
        _ => 0,
    }
}

fn apply_operator(values: &mut Vec<f64>, operator: char) -> Result<(), ExpressionError> {
    let Some(right) = values.pop() else {
        return Err(ExpressionError::new(format!("运算符 {operator} 缺少数字")));
    };
    let Some(left) = values.pop() else {
        return Err(ExpressionError::new(format!("运算符 {operator} 缺少数字")));
    };

    let value = match operator {
        '+' => left + right,
        '-' => left - right,
        '*' => left * right,
        '/' => {
            if right.abs() < EPSILON {
                return Err(ExpressionError::new("不能除以 0"));
            }
            left / right
        }
        _ => return Err(ExpressionError::new(format!("不支持的运算符：{operator}"))),
    };
    values.push(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(input: &str) -> f64 {
        evaluate(input).unwrap().value
    }

    #[test]
    fn precedence_and_parentheses() {
        assert!((value("1+2*3") - 7.0).abs() < EPSILON);
        assert!((value("(1+2)*8") - 24.0).abs() < EPSILON);
        assert!((value("6/(1+2)") - 2.0).abs() < EPSILON);
    }

    #[test]
    fn records_numbers() {
        let result = evaluate("(1+2)*(3+5)").unwrap();
        assert!((result.value - 24.0).abs() < EPSILON);
        assert_eq!(result.numbers, vec![1, 2, 3, 5]);
    }

    #[test]
    fn removes_backslashes_and_whitespace() {
        assert!((value("\\6 \\+ \\6 \\+ \\6 \\+ \\6") - 24.0).abs() < EPSILON);
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        for input in ["", "1+", "(", "1/0", "1+2)", "abc", "1+2*"] {
            assert!(evaluate(input).is_err(), "input should fail: {input}");
        }
    }
}
