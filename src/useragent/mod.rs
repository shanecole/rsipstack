use wasm_bindgen::prelude::*;

use crate::error::Error;


#[derive(Debug, PartialEq, Eq)]
#[wasm_bindgen]
pub struct UserAgent {
}

#[wasm_bindgen]
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Call {
    id: String,
    caller: String,
    callee: String,
    answer: String,
    offer: String,
}

#[wasm_bindgen]
impl UserAgent {
    #[wasm_bindgen(constructor)]
    pub fn new() -> UserAgent {
        UserAgent {}
    }
    
    pub async fn call(&self, _callee: &str, offer: &str) ->Result<Call, Error>{
        Ok(Call{
            callee: _callee.to_string(),
            offer: offer.to_string(),
            ..Default::default()
        })
    }
}
