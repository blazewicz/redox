use super::*;
use redox::*;

/// Get the next instruction
impl Editor {
    pub fn next_inst(&mut self) -> Inst {
        let mut n = 0;
        let mut shifted = false;

        // TODO: Make the switch to normal mode shift more well-coded.
        loop {
            if let EventOption::Key(k) = self.window.poll().unwrap_or(Event::new()).to_option() {
                let c = k.character;
                match c {
                    '\0' => {
                        return Inst(0, match k.scancode {
                            K_ALT => Key::Alt(k.pressed),
                            K_CTRL => Key::Ctrl(k.pressed),
                            K_LEFT_SHIFT | K_RIGHT_SHIFT => Key::Shift(k.pressed),
                            s if k.pressed => match s {
                                K_BKSP => Key::Backspace,
                                K_LEFT => Key::Left,
                                K_RIGHT => Key::Right,
                                K_UP => Key::Up,
                                K_DOWN => Key::Down,
                                K_TAB => Key::Tab,
                                K_ESC => Key::Escape,
                                _ => Key::Unknown(s),
                            },
                            s => Key::Unknown(s),
                        })
                    }
                    _ => if k.pressed {
                        match self.cursor().mode {
                            Mode::Primitive(_) => {
                                return Inst(0, Key::Char(c));
                            },
                            Mode::Command(_) => {
                                n = match c {
                                    '0' if n != 0 => n * 10,
                                    '1'           => n * 10 + 1,
                                    '2'           => n * 10 + 2,
                                    '3'           => n * 10 + 3,
                                    '4'           => n * 10 + 4,
                                    '5'           => n * 10 + 5,
                                    '6'           => n * 10 + 6,
                                    '7'           => n * 10 + 7,
                                    '8'           => n * 10 + 8,
                                    '9'           => n * 10 + 9,
                                    _             => {

                                        return Inst(if n == 0 { 1 } else { n }, Key::Char(c));
                                    }
                                }
                            }
                        }

                    },
                }
            }
        }

        unreachable!()
    }
}
