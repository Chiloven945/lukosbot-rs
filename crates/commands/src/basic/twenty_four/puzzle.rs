use super::expression::{EPSILON, TARGET};
use crate::basic::random::RandomSource;

const MAX_ATTEMPTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Puzzle {
    pub nums: [i32; 4],
    pub solution: String,
}

pub fn generate(random: &dyn RandomSource) -> Puzzle {
    for _ in 0..MAX_ATTEMPTS {
        let mut nums = [0; 4];
        for number in &mut nums {
            let candidate = (random.next_f64() * 13.0).floor() as i32 + 1;
            *number = candidate.clamp(1, 13);
        }

        if let Some(solution) = find_expression(nums) {
            return Puzzle { nums, solution };
        }
    }

    fallback()
}

pub fn fallback() -> Puzzle {
    Puzzle {
        nums: [6, 6, 6, 6],
        solution: "(6+6+6+6)".to_owned(),
    }
}

pub fn find_expression(nums: [i32; 4]) -> Option<String> {
    let nodes = nums
        .iter()
        .map(|number| Node {
            value: f64::from(*number),
            expression: number.to_string(),
        })
        .collect();
    find(nodes)
}

#[derive(Clone)]
struct Node {
    value: f64,
    expression: String,
}

fn find(nodes: Vec<Node>) -> Option<String> {
    if nodes.len() == 1 {
        let node = &nodes[0];
        return ((node.value - TARGET).abs() < EPSILON).then(|| node.expression.clone());
    }

    for left_index in 0..nodes.len() {
        for right_index in 0..nodes.len() {
            if left_index == right_index {
                continue;
            }

            let mut remaining = Vec::with_capacity(nodes.len() - 1);
            for (index, node) in nodes.iter().enumerate() {
                if index != left_index && index != right_index {
                    remaining.push(node.clone());
                }
            }

            let left = &nodes[left_index];
            let right = &nodes[right_index];

            if left_index < right_index
                && let Some(result) = find(with_node(
                    &remaining,
                    left.value + right.value,
                    format!("({}+{})", left.expression, right.expression),
                ))
            {
                return Some(result);
            }

            if let Some(result) = find(with_node(
                &remaining,
                left.value - right.value,
                format!("({}-{})", left.expression, right.expression),
            )) {
                return Some(result);
            }

            if left_index < right_index
                && let Some(result) = find(with_node(
                    &remaining,
                    left.value * right.value,
                    format!("({}*{})", left.expression, right.expression),
                ))
            {
                return Some(result);
            }

            if right.value.abs() > EPSILON
                && let Some(result) = find(with_node(
                    &remaining,
                    left.value / right.value,
                    format!("({}/{})", left.expression, right.expression),
                ))
            {
                return Some(result);
            }
        }
    }

    None
}

fn with_node(remaining: &[Node], value: f64, expression: String) -> Vec<Node> {
    let mut nodes = remaining.to_vec();
    nodes.push(Node { value, expression });
    nodes
}

#[cfg(test)]
mod tests {
    use super::super::expression::evaluate;
    use super::*;
    use crate::basic::random::SeededRandom;

    #[test]
    fn known_puzzle_is_solvable() {
        let solution = find_expression([6, 6, 6, 6]).expect("solution");
        let evaluated = evaluate(&solution).expect("valid expression");
        assert!((evaluated.value - TARGET).abs() < EPSILON);
    }

    #[test]
    fn generated_puzzles_are_valid() {
        let random = SeededRandom::new(11);
        for _ in 0..32 {
            let puzzle = generate(&random);
            assert!(puzzle.nums.iter().all(|number| (1..=13).contains(number)));
            let evaluated = evaluate(&puzzle.solution).expect("valid solution");
            assert!((evaluated.value - TARGET).abs() < EPSILON);
            let mut expected = puzzle.nums;
            let mut used = evaluated.numbers;
            expected.sort_unstable();
            used.sort_unstable();
            assert_eq!(used, expected);
        }
    }

    #[test]
    fn unsolvable_random_stream_uses_fallback() {
        struct Zero;
        impl RandomSource for Zero {
            fn next_f64(&self) -> f64 {
                0.0
            }
        }

        let puzzle = generate(&Zero);
        // [1, 1, 1, 1] is unsolvable, so the bounded generator must use its fallback.
        assert_eq!(puzzle.nums, [6, 6, 6, 6]);
        assert_eq!(puzzle.solution, "(6+6+6+6)");
    }
}
