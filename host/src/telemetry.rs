//! cctl が送るテレメトリ行の解釈。
//!
//! 受信例: `ST t=12345 r=12.300/11.800 th=45.000/44.200 z=100.000/99.100 en=7 mode=RUN err=00`
//! - 各軸は `目標/実測`。単位は r/z が mm、th が deg。
//! - 制御が発散した場合、値は `nan` として送られてくる。
//! - `en` は有効な軸のビットマスク、`err` は z 軸ドライバのエラービット（16 進）。

/// 軸を指すビット。cctl 側の `domain::axis_bit` と対応する。
pub mod axis_bit {
    pub const R: u8 = 1 << 0;
    pub const THETA: u8 = 1 << 1;
    pub const Z: u8 = 1 << 2;
}

/// 機体の運転状態。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunMode {
    Safe,
    Run,
    Stop,
}

impl RunMode {
    pub fn label(self) -> &'static str {
        match self {
            RunMode::Safe => "SAFE（待機）",
            RunMode::Run => "RUN（運転）",
            RunMode::Stop => "STOP（非常停止）",
        }
    }
}

/// 1 軸分の目標値と実測値。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AxisState {
    pub target: f32,
    pub measured: f32,
}

impl AxisState {
    /// 追従誤差。どちらかが nan なら nan。
    pub fn error(&self) -> f32 {
        self.target - self.measured
    }
}

/// テレメトリ 1 行の内容。
#[derive(Clone, PartialEq, Debug)]
pub struct Telemetry {
    pub uptime_ms: u32,
    pub r: AxisState,
    pub theta: AxisState,
    pub z: AxisState,
    pub enabled_axes: u8,
    pub mode: RunMode,
    pub error_bits: u8,
}

impl Telemetry {
    pub fn axis_enabled(&self, bit: u8) -> bool {
        self.enabled_axes & bit != 0
    }
}

/// `目標/実測` を読む。
fn parse_axis(text: &str) -> Option<AxisState> {
    let (target, measured) = text.split_once('/')?;
    Some(AxisState {
        target: target.parse().ok()?,
        measured: measured.parse().ok()?,
    })
}

fn parse_mode(text: &str) -> Option<RunMode> {
    match text {
        "SAFE" => Some(RunMode::Safe),
        "RUN" => Some(RunMode::Run),
        "STOP" => Some(RunMode::Stop),
        _ => None,
    }
}

/// テレメトリ行を解釈する。形式が違えば `None`。
pub fn parse_telemetry(line: &str) -> Option<Telemetry> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "ST" {
        return None;
    }

    let mut uptime_ms = None;
    let mut r = None;
    let mut theta = None;
    let mut z = None;
    let mut enabled_axes = None;
    let mut mode = None;
    let mut error_bits = None;

    for token in tokens {
        let (key, value) = token.split_once('=')?;
        match key {
            "t" => uptime_ms = Some(value.parse().ok()?),
            "r" => r = Some(parse_axis(value)?),
            "th" => theta = Some(parse_axis(value)?),
            "z" => z = Some(parse_axis(value)?),
            "en" => enabled_axes = Some(value.parse().ok()?),
            "mode" => mode = Some(parse_mode(value)?),
            "err" => error_bits = Some(u8::from_str_radix(value, 16).ok()?),
            // 将来フィールドが増えても古い host が落ちないよう、知らない鍵は読み飛ばす。
            _ => {}
        }
    }

    Some(Telemetry {
        uptime_ms: uptime_ms?,
        r: r?,
        theta: theta?,
        z: z?,
        enabled_axes: enabled_axes?,
        mode: mode?,
        error_bits: error_bits?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "ST t=12345 r=12.300/11.800 th=45.000/44.200 z=100.000/99.100 en=7 mode=RUN err=00";

    #[test]
    fn parses_all_fields() {
        let t = parse_telemetry(SAMPLE).expect("解釈できること");

        assert_eq!(t.uptime_ms, 12345);
        assert_eq!(t.r.target, 12.3);
        assert_eq!(t.r.measured, 11.8);
        assert_eq!(t.theta.target, 45.0);
        assert_eq!(t.z.measured, 99.1);
        assert_eq!(t.enabled_axes, 7);
        assert_eq!(t.mode, RunMode::Run);
        assert_eq!(t.error_bits, 0);
    }

    #[test]
    fn parses_negative_values() {
        let t = parse_telemetry(
            "ST t=1 r=-1.500/-1.250 th=-180.000/0.000 z=0.000/0.000 en=0 mode=SAFE err=00",
        )
        .expect("解釈できること");

        assert_eq!(t.r.target, -1.5);
        assert_eq!(t.theta.target, -180.0);
    }

    #[test]
    fn parses_nan_as_nan() {
        // 発散を数値に丸めずそのまま表示するため。
        let t = parse_telemetry(
            "ST t=1 r=nan/nan th=0.000/0.000 z=0.000/0.000 en=1 mode=RUN err=00",
        )
        .expect("解釈できること");

        assert!(t.r.target.is_nan());
        assert!(t.r.measured.is_nan());
    }

    #[test]
    fn parses_modes() {
        for (text, expected) in [
            ("SAFE", RunMode::Safe),
            ("RUN", RunMode::Run),
            ("STOP", RunMode::Stop),
        ] {
            let line = SAMPLE.replace("mode=RUN", &format!("mode={text}"));
            assert_eq!(parse_telemetry(&line).unwrap().mode, expected);
        }
    }

    #[test]
    fn parses_hex_error_bits() {
        let line = SAMPLE.replace("err=00", "err=0A");
        assert_eq!(parse_telemetry(&line).unwrap().error_bits, 0x0A);
    }

    #[test]
    fn reports_enabled_axes() {
        let line = SAMPLE.replace("en=7", "en=2");
        let t = parse_telemetry(&line).unwrap();

        assert!(!t.axis_enabled(axis_bit::R));
        assert!(t.axis_enabled(axis_bit::THETA));
        assert!(!t.axis_enabled(axis_bit::Z));
    }

    #[test]
    fn ignores_unknown_keys() {
        // ファームが新しくても host を落とさない。
        let line = format!("{SAMPLE} foo=1");
        assert!(parse_telemetry(&line).is_some());
    }

    #[test]
    fn rejects_other_lines() {
        assert!(parse_telemetry("").is_none());
        assert!(parse_telemetry("LX+000 LY+000").is_none());
        assert!(parse_telemetry("STOP").is_none());
    }

    #[test]
    fn rejects_incomplete_lines() {
        assert!(parse_telemetry("ST t=1 r=0.000/0.000").is_none());
        assert!(parse_telemetry(&SAMPLE.replace("mode=RUN", "mode=GO")).is_none());
        assert!(parse_telemetry(&SAMPLE.replace("r=12.300/11.800", "r=12.300")).is_none());
    }

    #[test]
    fn computes_following_error() {
        let t = parse_telemetry(SAMPLE).unwrap();
        assert!((t.r.error() - 0.5).abs() < 1e-4);
    }
}
