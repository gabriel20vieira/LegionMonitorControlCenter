//! DDC/CI feature table. Codes and value meanings come from the original
//! program's VCPCodeDef.json (service MSI) — see MECHANICS.md.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Image,
    Game,
    Input,
}

pub enum Kind {
    /// Continuous 0..max slider.
    Slider,
    /// Named values.
    Choice(&'static [(u32, &'static str)]),
}

pub struct Feature {
    pub code: u8,
    pub name: &'static str,
    pub tab: Tab,
    pub kind: Kind,
}

pub const BRIGHTNESS: u8 = 0x10;
pub const CONTRAST: u8 = 0x12;
pub const RESTORE_FACTORY: u8 = 0x04;

const COLOR_PRESET: &[(u32, &str)] = &[
    (0x01, "sRGB"),
    (0x02, "Nativo do ecrã"),
    (0x05, "Avermelhado (6500K)"),
    (0x06, "Neutro (7500K)"),
    (0x08, "Azulado (9300K)"),
    (0x0B, "Personalizado 1"),
    (0x0C, "Personalizado 2"),
    (0x0D, "Personalizado 3"),
    (0x0E, "BT.709"),
    (0x0F, "DCI-P3"),
    (0x10, "BT.2020"),
    (0x11, "Adobe RGB"),
    (0x12, "Luz azul reduzida"),
    (0x13, "DICOM"),
];

const SCENARIO: &[(u32, &str)] = &[
    (0x00, "Nativo do painel"),
    (0x01, "Criação de imagem"),
    (0x03, "Cinema digital"),
    (0x05, "Criação de vídeo"),
    (0x06, "Luz azul reduzida"),
    (0x07, "Navegação web"),
    (0x08, "Jogos"),
    (0x09, "Imagem profissional"),
];

const SCALING: &[(u32, &str)] = &[
    (0x01, "Sem escala"),
    (0x02, "Ecrã inteiro"),
    (0x05, "Proporção original"),
    (0x0B, "4:3"),
    (0x0F, "16:9"),
    (0x10, "16:10"),
];

const OVERDRIVE: &[(u32, &str)] = &[
    (0x00, "Desligado"),
    (0x01, "Normal"),
    (0x02, "Extremo"),
    (0x03, "Nível 1"),
    (0x04, "Nível 2"),
    (0x05, "Nível 3"),
    (0x06, "Nível 4"),
    (0x07, "Nível 5"),
];

const ON_OFF: &[(u32, &str)] = &[(0x00, "Desligado"), (0x01, "Ligado")];

const GAME_MODE: &[(u32, &str)] = &[
    (0x00, "Desligado"),
    (0x01, "FPS 1"),
    (0x02, "FPS 2"),
    (0x03, "Corrida (RCG)"),
    (0x04, "Estratégia (RTS)"),
    (0x05, "Jogo 1"),
    (0x06, "Jogo 2"),
    (0x07, "MOBA"),
    (0x08, "Ação (ACT)"),
    (0x09, "RPG"),
    (0x0A, "Padrão"),
];

const HDR: &[(u32, &str)] = &[
    (0x00, "Desligado"),
    (0x01, "Automático"),
    (0x02, "HDR1000"),
    (0x03, "HLG"),
    (0x04, "Dolby Vision"),
    (0x05, "HDR"),
    (0x06, "HDR400"),
    (0x07, "HDR Jogo"),
    (0x08, "HDR Filme"),
    (0x09, "HDR Foto"),
    (0x0C, "HDR10"),
];

pub const INPUT: &[(u32, &str)] = &[
    (0x0F, "DisplayPort 1"),
    (0x10, "DisplayPort 2"),
    (0x11, "HDMI 1"),
    (0x12, "HDMI 2"),
    (0x13, "HDMI 3"),
    (0x14, "HDMI 4"),
    (0x31, "USB-C 1"),
    (0x32, "USB-C 2"),
    (0x01, "VGA (padrão MCCS)"),
    (0x03, "DVI 1"),
    (0x04, "DVI 2"),
    (0xFE, "Automático"),
];

const POWER: &[(u32, &str)] = &[(0x01, "Ligado"), (0x04, "Em espera"), (0x05, "Desligado")];

pub const FEATURES: &[Feature] = &[
    Feature { code: BRIGHTNESS, name: "Brilho", tab: Tab::Image, kind: Kind::Slider },
    Feature { code: CONTRAST, name: "Contraste", tab: Tab::Image, kind: Kind::Slider },
    Feature { code: 0x14, name: "Predefinição de cor", tab: Tab::Image, kind: Kind::Choice(COLOR_PRESET) },
    Feature { code: 0xDC, name: "Modo de cenário", tab: Tab::Image, kind: Kind::Choice(SCENARIO) },
    Feature { code: 0x16, name: "Ganho vermelho", tab: Tab::Image, kind: Kind::Slider },
    Feature { code: 0x18, name: "Ganho verde", tab: Tab::Image, kind: Kind::Slider },
    Feature { code: 0x1A, name: "Ganho azul", tab: Tab::Image, kind: Kind::Slider },
    Feature { code: 0x86, name: "Escala da imagem", tab: Tab::Image, kind: Kind::Choice(SCALING) },
    Feature { code: 0xE0, name: "Over Drive", tab: Tab::Game, kind: Kind::Choice(OVERDRIVE) },
    Feature { code: 0xEA, name: "Contraste dinâmico", tab: Tab::Game, kind: Kind::Choice(ON_OFF) },
    Feature { code: 0xF9, name: "Modo de jogo", tab: Tab::Game, kind: Kind::Choice(GAME_MODE) },
    Feature { code: 0xEF, name: "HDR", tab: Tab::Game, kind: Kind::Choice(HDR) },
    Feature { code: 0x60, name: "Fonte de entrada", tab: Tab::Input, kind: Kind::Choice(INPUT) },
    Feature { code: 0xD6, name: "Energia", tab: Tab::Input, kind: Kind::Choice(POWER) },
];

/// Codes read when a monitor is selected (in this order: most used first).
pub fn read_order() -> Vec<u8> {
    FEATURES.iter().map(|f| f.code).collect()
}

/// Parsed `vcp(...)` part of a monitor's MCCS capabilities string.
#[derive(Clone, Debug, Default)]
pub struct Caps {
    #[allow(dead_code)]
    pub raw: String,
    pub model: Option<String>,
    /// code → allowed values (empty = continuous / not listed)
    pub vcp: std::collections::BTreeMap<u8, Vec<u32>>,
}

impl Caps {
    pub fn supports(&self, code: u8) -> bool {
        self.vcp.is_empty() || self.vcp.contains_key(&code)
    }
    pub fn values(&self, code: u8) -> Option<&Vec<u32>> {
        self.vcp.get(&code).filter(|v| !v.is_empty())
    }
}

fn section<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let lower = s.to_ascii_lowercase();
    let start = lower.find(&format!("{name}("))? + name.len() + 1;
    let mut depth = 1;
    for (i, c) in s[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..start + i]);
                }
            }
            _ => {}
        }
    }
    Some(&s[start..])
}

pub fn parse_caps(raw: &str) -> Caps {
    let mut caps = Caps { raw: raw.to_string(), ..Default::default() };
    caps.model = section(raw, "model").map(|s| s.trim().to_string());
    if let Some(v) = section(raw, "vcp") {
        let bytes: Vec<char> = v.chars().collect();
        let mut i = 0;
        let mut last: Option<u8> = None;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_hexdigit() {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                    j += 1;
                }
                let tok: String = bytes[i..j].iter().collect();
                if let Ok(code) = u8::from_str_radix(&tok[..tok.len().min(2)], 16) {
                    caps.vcp.entry(code).or_default();
                    last = Some(code);
                }
                i = j;
            } else if c == '(' {
                let mut j = i + 1;
                let mut depth = 1;
                while j < bytes.len() && depth > 0 {
                    if bytes[j] == '(' {
                        depth += 1;
                    } else if bytes[j] == ')' {
                        depth -= 1;
                    }
                    j += 1;
                }
                let inner: String = bytes[i + 1..j.saturating_sub(1)].iter().collect();
                if let Some(code) = last {
                    let vals: Vec<u32> = inner
                        .split(|ch: char| !ch.is_ascii_hexdigit())
                        .filter(|t| !t.is_empty())
                        .filter_map(|t| u32::from_str_radix(t, 16).ok())
                        .collect();
                    caps.vcp.insert(code, vals);
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    #[test]
    fn caps() {
        let c = super::parse_caps("(prot(monitor)type(LCD)model(Y27q)cmds(01 02)vcp(02 04 10 12 14(05 06 08 0B) 60(0F 11 12) E0)mccs_ver(2.2))");
        assert_eq!(c.model.as_deref(), Some("Y27q"));
        assert_eq!(c.values(0x60).unwrap(), &vec![0x0F, 0x11, 0x12]);
        assert!(c.supports(0x10));
        assert!(!c.supports(0xEF));
    }
}
