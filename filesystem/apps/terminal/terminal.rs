use redox::ops::DerefMut;
use redox::string::*;
use redox::vec::Vec;
use redox::boxed::Box;
use redox::fs::*;
use redox::io::*;
use redox::console::*;
use redox::env::*;
use redox::time::Duration;
use redox::to_num::*;

/* Magic Macros { */
static mut application: *mut Application<'static> = 0 as *mut Application;

/// Execute a command
macro_rules! exec {
    ($cmd:expr) => ({
        unsafe {
            (*application).on_command(&$cmd.to_string());
        }
    })
}
/* } Magic Macros */

/// A command
pub struct Command<'a> {
    pub name: &'a str,
    pub main: Box<Fn(&Vec<String>)>,
}

impl<'a> Command<'a> {
    /// Return the vector of the commands
    // TODO: Use a more efficient collection instead
    pub fn vec() -> Vec<Self> {
        let mut commands: Vec<Self> = Vec::new();
        commands.push(Command {
            name: "echo",
            main: box |args: &Vec<String>| {
                let echo = args.iter()
                    .skip(1)
                    .fold(String::new(), |string, arg| string + " " + arg);
                println!("{}", echo.trim());
            },
        });

        commands.push(Command {
            name: "open",
            main: box |args: &Vec<String>| {
                if let Some(arg) = args.get(1) {
                    File::exec(arg);
                }
            },
        });

        commands.push(Command {
            name: "run",
            main: box |args: &Vec<String>| {
                if let Some(path) = args.get(1) {

                    let mut commands = String::new();
                    if let Some(mut file) = File::open(path) {
                        println!("URL: {:?}", file.path());

                        file.read_to_string(&mut commands);
                    }

                    for command in commands.split('\n') {
                        exec!(command);
                    }
                }
            },
        });

        commands.push(Command {
            name: "send",
            main: box |args: &Vec<String>| {
                if args.len() < 3 {
                    println!("Error: incorrect arguments");
                    println!("Usage: send [url] [data]");
                    return;
                }

                let path = {
                    match args.get(1) {
                        Some(arg) => arg.clone(),
                        None => String::new(),
                    }
                };

                if let Some(mut file) = File::open(&path) {
                    println!("URL: {:?}", file.path());

                    let string: String = args.iter()
                        .skip(2)
                        .fold(String::new(), |s, arg| s + " " + arg)
                        + "\r\n\r\n";

                    match file.write(string.trim_left().as_bytes()) {
                        Some(size) => println!("Wrote {} bytes", size),
                        None => println!("Failed to write"),
                    }

                    let mut string = String::new();
                    match file.read_to_string(&mut string) {
                        Some(_) => println!("{}", string),
                        None => println!("Failed to read"),
                    }
                }
            },
        });

        commands.push(Command {
            name: "sleep",
            main: box |args: &Vec<String>| {
                let secs = {
                    match args.get(1) {
                        Some(arg) => arg.to_num() as i64,
                        None => 0,
                    }
                };

                let nanos = {
                    match args.get(2) {
                        Some(arg) => arg.to_num() as i32,
                        None => 0,
                    }
                };

                println!("Sleep: {} {}", secs, nanos);
                let remaining = Duration::new(secs, nanos).sleep();
                println!("Remaining: {} {}", remaining.secs, remaining.nanos);
            },
        });

        commands.push(Command {
            name: "url",
            main: box |args: &Vec<String>| {
                let path = {
                    match args.get(1) {
                        Some(arg) => arg.clone(),
                        None => String::new(),
                    }
                };

                if let Some(mut file) = File::open(&path) {
                    println!("URL: {:?}", file.path());

                    let mut string = String::new();
                    match file.read_to_string(&mut string) {
                        Some(_) => println!("{}", string),
                        None => println!("Failed to read"),
                    }
                }
            },
        });

        commands.push(Command {
            name: "url_hex",
            main: box |args: &Vec<String>| {
                let path = {
                    match args.get(1) {
                        Some(arg) => arg.clone(),
                        None => String::new(),
                    }
                };

                if let Some(mut file) = File::open(&path) {
                    println!("URL: {:?}", file.path());

                    let mut vec: Vec<u8> = Vec::new();
                    match file.read_to_end(&mut vec) {
                        Some(_) => {
                            let mut line = "HEX:".to_string();
                            for byte in vec.iter() {
                                line = line + " " + &format!("{:X}", *byte);
                            }
                            println!("{}", line);
                        }
                        None => println!("Failed to read"),
                    }
                }
            },
        });

        commands.push(Command {
            name: "wget",
            main: box |args: &Vec<String>| {
                if let Some(host) = args.get(1) {
                    if let Some(req) = args.get(2) {
                        if let Some(mut con) = File::open(&("tcp://".to_string() + host)) {
                            con.write(("GET ".to_string() + req + " HTTP/1.1").as_bytes());

                            let mut res = Vec::new();
                            con.read_to_end(&mut res);

                            if let Some(mut file) = File::open(&req) {
                                file.write(&res);
                            }
                        }
                    } else {
                        println!("No request given");
                    }
                } else {
                    println!("No url given");
                }
            },
        });

        let command_list = commands.iter().fold(String::new(), |l , c| l + " " + c.name) + " exit";

        commands.push(Command {
            name: "help",
            main: box move |args: &Vec<String>| {
                println!("Commands:{}", command_list);
            },
         });

        commands
    }
}

/// A (env) variable
pub struct Variable {
    pub name: String,
    pub value: String,
}

pub struct Mode {
    value: bool,
}

/// An application
pub struct Application<'a> {
    commands: Vec<Command<'a>>,
    variables: Vec<Variable>,
    modes: Vec<Mode>,
}

impl<'a> Application<'a> {
    /// Create a new empty application
    pub fn new() -> Self {
        return Application {
            commands: Command::vec(),
            variables: Vec::new(),
            modes: Vec::new(),
        };
    }

    fn on_command(&mut self, command_string: &str) {
        //Comment
        if command_string.starts_with('#') {
            return;
        }

        //Show variables
        if command_string == "$" {
            let variables = self.variables.iter()
                .fold(String::new(),
                      |string, variable| string + "\n" + &variable.name + "=" + &variable.value);
            println!("{}", variables);
            return;
        }

        //Explode into arguments, replace variables
        let mut args: Vec<String> = Vec::<String>::new();
        for arg in command_string.split(' ') {
            if !arg.is_empty() {
                if arg.starts_with('$') {
                    let name = arg[1 .. arg.len()].to_string();
                    for variable in self.variables.iter() {
                        if variable.name == name {
                            args.push(variable.value.clone());
                            break;
                        }
                    }
                } else {
                    args.push(arg.to_string());
                }
            }
        }

        //Execute commands
        if let Some(cmd) = args.get(0) {
            if cmd == "if" {
                let mut value = false;

                if let Some(left) = args.get(1) {
                    if let Some(cmp) = args.get(2) {
                        if let Some(right) = args.get(3) {
                            if cmp == "==" {
                                value = *left == *right;
                            } else if cmp == "!=" {
                                value = *left != *right;
                            } else if cmp == ">" {
                                value = left.to_num_signed() > right.to_num_signed();
                            } else if cmp == ">=" {
                                value = left.to_num_signed() >= right.to_num_signed();
                            } else if cmp == "<" {
                                value = left.to_num_signed() < right.to_num_signed();
                            } else if cmp == "<=" {
                                value = left.to_num_signed() <= right.to_num_signed();
                            } else {
                                println!("Unknown comparison: {}", cmp);
                            }
                        }
                    }
                }

                self.modes.insert(0, Mode { value: value });
                return;
            }

            if cmd == "else" {
                let mut syntax_error = false;
                match self.modes.get_mut(0) {
                    Some(mode) => mode.value = !mode.value,
                    None => syntax_error = true,
                }
                if syntax_error {
                    println!("Syntax error: else found with no previous if");
                }
                return;
            }

            if cmd == "fi" {
                let mut syntax_error = false;
                if !self.modes.is_empty() {
                    self.modes.remove(0);
                } else {
                    syntax_error = true;
                }
                if syntax_error {
                    println!("Syntax error: fi found with no previous if");
                }
                return;
            }

            for mode in self.modes.iter() {
                if !mode.value {
                    return;
                }
            }

            //Set variables
            if let Some(i) = cmd.find('=') {
                let name = cmd[0 .. i].to_string();
                let mut value = cmd[i + 1 .. cmd.len()].to_string();

                if name.is_empty() {
                    return;
                }

                for i in 1..args.len() {
                    if let Some(arg) = args.get(i) {
                        value = value + " " + &arg;
                    }
                }

                if value.is_empty() {
                    let mut remove = -1;
                    for i in 0..self.variables.len() {
                        match self.variables.get(i) {
                            Some(variable) => if variable.name == name {
                                remove = i as isize;
                                break;
                            },
                            None => break,
                        }
                    }

                    if remove >= 0 {
                        self.variables.remove(remove as usize);
                    }
                } else {
                    for variable in self.variables.iter_mut() {
                        if variable.name == name {
                            variable.value = value;
                            return;
                        }
                    }

                    self.variables.push(Variable {
                        name: name,
                        value: value,
                    });
                }
                return;
            }

            //Commands
            for command in self.commands.iter() {
                if &command.name == cmd {
                    (*command.main)(&args);
                    return;
                }
            }

            println!("Unknown command: '{}'", cmd);
        }
    }

    /// Run the application
    pub fn main(&mut self) {
        console_title("Terminal");

        println!("Type help for a command list");
        if let Some(arg) = args().get(1) {
            let command = "run ".to_string() + arg;
            println!("# {}", command);
            self.on_command(&command);
        }

        while let Some(command) = readln!() {
            println!("# {}", command);
            if command == "exit" {
                break;
            } else if !command.is_empty() {
                self.on_command(&command);
            }
        }
    }
}

pub fn main() {
    unsafe {
        let mut app = box Application::new();
        application = app.deref_mut();
        app.main();
    }
}
