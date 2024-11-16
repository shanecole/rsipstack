
use futures::StreamExt;
use rsipstack:: EndpointBuilder;
use tracing::info;

// A sip client example, that sends a REGISTER request to a sip server.
#[tokio::main]
async fn main() {
    let user_agent = EndpointBuilder::new().build();
    let client_loop = async {
        let register_req = rsip::message::Request{
            method: rsip::method::Method::Register,
            uri: rsip::Uri {
                scheme: Some(rsip::Scheme::Sips),
                host_with_port: rsip::Domain::from("example.com").into(),
                ..Default::default()
            },
            headers: rsip::Headers::default(),
            version: rsip::Version::V2,
            body: Default::default(),
        };
        let tx = user_agent.client_transaction(register_req);
        tx.send().await.expect("Failed to send register request.");
        while let Some(resp) = tx.next().await {
            info!("Received response: {:?}", resp);
        }
    };
    
    let server_loop = async {
        while let Some(_tx) = user_agent.server_transaction().next().await {
        }
    };

    tokio::select! {
        _ = user_agent.serve() => {},
        _ = client_loop => {},
        _ = server_loop => {},
    }

    // let register_tx = user_agent.client_transaction(method::Method::Register)
    // .on_challenge(|response| {
    //     let www_authenticate = response.headers.get::<www_authenticate::WwwAuthenticate>().unwrap();
    //     let user = response.uri.user().unwrap();
    //     let password = "password";
    //     let req = response.create_auth_request(user, password);
    //     req
    // })
    // .send().await?;

    // while let Some(response) = register_tx.next().await {
    //     match response {
    //         response::Response::Final(response) => {
    //             if response.status_code == 200 {
    //                 println!("Successfully registered to the sip server.");
    //             } else {
    //                 println!("Failed to register to the sip server.");
    //             }
    //         }
    //         response::Response::Provisional(response) => {
    //             println!("Received provisional response: {:?}", response);
    //         }
    //     }
    // }
    
    // let invite_tx = user_agent.client_transaction(method::Method::Invite);
    // let invite_tx = invite_tx.send().await?;

    // while let Some(response) = invite_tx.next().await {
    //     match response {
    //         response::Response::Final(response) => {
    //             let body = response.body();
    //             invite_tx.send_ack().await?;
    //             break
    //         }
    //         response::Response::Provisional(response) => {
    //             println!("Received provisional response: {:?}", response);
    //         }
    //     }
    // }
    
    // let bye_tx = invite_tx.client_transaction(method::Method::Bye);
    // let bye_tx = bye_tx.send().await?;
    // // let bye_tx = user_agent.client_transaction(method::Method::Bye).await
    // // .to(invite_tx.to());
    // // let bye_tx = bye_tx.send().await?;

}