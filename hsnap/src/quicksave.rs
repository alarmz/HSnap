//! QuickSave file naming: numeric-suffix auto increment, printf-style `%04d`, looping.

#[derive(Clone, Debug)]
pub struct QuickSave {
    /// File name template, e.g. `Capture001.png`, `Snap%04d.gif`.
    pub template: String,
    pub start: i64,
    /// "stop saving at" number as typed; its length sets the zero padding.
    pub stop: Option<String>,
    pub step: i64,
    pub looping: bool,
    counter: Option<i64>,
}

impl QuickSave {
    pub fn new(template: &str, start: i64, stop: Option<&str>, step: i64, looping: bool) -> Self {
        Self { template: template.into(), start, stop: stop.map(String::from), step: step.max(1), looping, counter: None }
    }

    fn stop_value(&self) -> Option<i64> {
        self.stop.as_ref().and_then(|s| s.parse().ok()).filter(|v: &i64| *v > 0) // 0 = no limit
    }

    fn width(&self) -> usize {
        match &self.stop {
            Some(s) if s.chars().all(|c| c == '0') => s.len() + 1, // "000" -> 4 digits
            Some(s) => s.len(),
            None => 0,
        }
    }

    /// Next file name, or `None` once the stop number is passed (when not looping).
    pub fn next_name(&mut self) -> Option<String> {
        let n = match self.counter {
            None => self.start,
            Some(c) => c + self.step,
        };
        let n = match self.stop_value() {
            Some(stop) if n > stop => {
                if self.looping { self.start } else { return None }
            }
            _ => n,
        };
        self.counter = Some(n);
        Some(render(&self.template, n, self.width()))
    }
}

fn render(template: &str, n: i64, width: usize) -> String {
    if let Some((pre, w, post)) = split_printf(template) {
        return format!("{pre}{n:0w$}{post}");
    }
    let (stem, ext) = match template.rfind('.') {
        Some(i) => template.split_at(i),
        None => (template, ""),
    };
    // Last run of digits in the stem.
    let bytes = stem.as_bytes();
    let end = match bytes.iter().rposition(|b| b.is_ascii_digit()) {
        Some(e) => e + 1,
        None => return format!("{stem}{n:0width$}{ext}"),
    };
    let mut begin = end;
    while begin > 0 && bytes[begin - 1].is_ascii_digit() {
        begin -= 1;
    }
    format!("{}{n:0width$}{}{ext}", &stem[..begin], &stem[end..])
}

/// Finds `%d` / `%0Nd` and returns (before, width, after).
fn split_printf(t: &str) -> Option<(&str, usize, &str)> {
    let i = t.find('%')?;
    let rest = &t[i + 1..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !rest[digits.len()..].starts_with('d') {
        return None;
    }
    let w = digits.trim_start_matches('0').parse().unwrap_or(0);
    Some((&t[..i], w, &rest[digits.len() + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_last_numeric_group() {
        let mut q = QuickSave::new("Img123-017.bmp", 18, None, 1, false);
        assert_eq!(q.next_name().unwrap(), "Img123-18.bmp");
        assert_eq!(q.next_name().unwrap(), "Img123-19.bmp");
    }

    #[test]
    fn stop_number_controls_padding() {
        let mut q = QuickSave::new("Img123-017.bmp", 18, Some("99"), 1, false);
        assert_eq!(q.next_name().unwrap(), "Img123-18.bmp");
        let mut q = QuickSave::new("Img123-017.bmp", 18, Some("000"), 1, false);
        assert_eq!(q.next_name().unwrap(), "Img123-0018.bmp");
    }

    #[test]
    fn printf_takes_precedence() {
        let mut q = QuickSave::new("Snap%04d.gif", 1, None, 1, false);
        assert_eq!(q.next_name().unwrap(), "Snap0001.gif");
        assert_eq!(q.next_name().unwrap(), "Snap0002.gif");
    }

    #[test]
    fn stops_or_loops() {
        let mut q = QuickSave::new("a1.png", 1, Some("2"), 1, false);
        assert!(q.next_name().is_some() && q.next_name().is_some());
        assert!(q.next_name().is_none());
        let mut q = QuickSave::new("a1.png", 1, Some("2"), 1, true);
        let names: Vec<_> = (0..4).map(|_| q.next_name().unwrap()).collect();
        assert_eq!(names, ["a1.png", "a2.png", "a1.png", "a2.png"]);
    }

    #[test]
    fn step_and_no_digits() {
        let mut q = QuickSave::new("shot.png", 1, None, 5, false);
        assert_eq!(q.next_name().unwrap(), "shot1.png");
        assert_eq!(q.next_name().unwrap(), "shot6.png");
    }
}
