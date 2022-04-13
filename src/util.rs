// mirra (c) Nikolas Wipper 2022

use std::fmt::Debug;
use std::io::Result;
use std::str::FromStr;

use dialoguer::Input;

pub fn simple_input<S: Into<String>, T>(prompt: S) -> Result<T>
    where
        T: Clone + ToString + FromStr,
        <T as FromStr>::Err: Debug + ToString {
    Input::new()
        .with_prompt(prompt)
        .interact_text()
}

pub fn simple_input_default<S: Into<String>, T>(prompt: S, default: T) -> Result<T>
    where
        T: Clone + ToString + FromStr,
        <T as FromStr>::Err: Debug + ToString {
    Input::new()
        .with_prompt(prompt)
        .default(default)
        .interact_text()
}
