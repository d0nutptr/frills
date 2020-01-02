# `frills`
*A no-frills, simple message queue system aimed at stability, high throughput, and simplicity.*

## What is `frills`?
`frills` is fairly straightforward to use. It lacks a lot of features but that's also supposed to be defining point of it. I try to make 0 guarantees about anything and you should only use `frills` for local testing or toy projects.

A list of some features that are missing (currently):

1. Proper handling on client disconnect (_I'm probably going to actually add this..._)
2. Cluster support
3. Flexible routing
4. Languagues other than Rust
5. Literally any UI
6. AuthN/Z
7. Event order guarantees
8. _and many more!!1!_

This thing has some issues too! For example, if no messages are transmitted, the clients frantically call out into the void asking for more messages (and the server responds with an empty message set..) which causes some high cpu usage. It's not hard to fix, I'm just lazy and haven't done it yet. Basically, I'd un-fuck the server code that I messed up while debugging an issue where I axe'd the whole "Pending Clients" idea which would allow clients to ask for a message and then CHILL-OUT until a message finally hit the queue. 

## Example
`server.rs - 1.2.3.4`
```rust
#[tokio::main]
async fn main() {
  let mut server = FrillsServer::new(12345);
  server.run().await;
}
```

`dog_name_client.rs`
```rust
#[tokio::main]
async fn main() {
  let mut dog_client = FrillsClient::builder("DogClient")
      .remote_from_str("127.0.0.1:12345")
      .cache_size(1)
      .build().await
      .unwrap();
  let mut dog_handle = dog_client.get_client_handle();

  dog_handle.register_topic("DogNames").await;
  dog_handle.register_topic("l33tPuppyNames").await;
  dog_handle.subscribe_to_topic("l33tPuppyNames").await;

  // to ensure all of the topics are registered properly
  tokio::time::delay_for(Duration::from_millis(250)).await;

  // let's push out some dog names
  let dog_names = vec!["fido", "rufus", "taiko"];
  dog_handle.push_messages("DogNames", dog_names.into_iter().map(|name| name.as_bytes().to_vec()).collect()).await;

  // damned async closures don't work yet; gotta use `while let`
  // for more speed, convert this to be concurrent by pulling additional messages
  // and executing your tasks in parallel with join_all
  while let Some(message) = dog_client.next().await {
      let puppy_name = String::from_utf8(message.message).unwrap();
      let dog_name = puppy_name.clone().replace("1", "i")
        .replace("0", "o")
        .replace("5", "s")
        .replace("@", "a");

      println!("(Dog Client): Converted {} to {}", puppy_name, dog_name);

      dog_handle.push_messages("DogNames", vec![dog_name.into_bytes().to_vec()]).await
  }
}
```

`l33t_puppy_name_client.rs`
```rust
#[tokio::main]
async fn main() {
  let mut puppy_client = FrillsClient::builder("l33tPuppyClient")
      .remote_from_str("127.0.0.1:12345")
      .cache_size(1)
      .build().await
      .unwrap();
  let mut puppy_handle = puppy_client.get_client_handle();

  puppy_handle.register_topic("DogNames").await;
  puppy_handle.register_topic("l33tPuppyNames").await;
  puppy_handle.subscribe_to_topic("DogNames").await;

  // to ensure all of the topics are registered properly
  tokio::time::delay_for(Duration::from_millis(250)).await;

  // damned async closures don't work yet; gotta use `while let`
  // for more speed, convert this to be concurrent by pulling additional messages
  // and executing your tasks in parallel with join_all
  while let Some(message) = puppy_client.next().await {
      let dog_name = String::from_utf8(message.message).unwrap();
      let puppy_name = dog_name.clone().replace("i", "1")
        .replace("o", "0")
        .replace("s", "5")
        .replace("a", "@");

      println!("(133t Puppy Client): Converted {} to {}", dog_name, puppy_name);

      puppy_handle.push_messages("l33tPuppyNames", vec![puppy_name.into_bytes().to_vec()]).await
  }
}
```

The above example code is available as a test case in `tests.rs` by running `cargo test test_puppy -- --nocapture`

## How does routing work?
`frills` takes a very opinionated approach to message routing.

1. Clients register as a **Service** to `frills`
  * Multiple instances can register as the same **Service**. This will distribute the load of that **Service**'s queue among all instances opf that **Service**
2. **Services** subscribe to **Topics**
  * **Services** can subscribe to more than one **Topic**. When a **Service** asks for a message, it will be given a collection of messages from all **Topics** 
3. **Messages** are pushed to **Topics**
  * **Messages** are distributed to all **Service** queues subscribed to a topic.
    * If **Service** "S1" and "S2" are subscribed to **Topic** "MyTopic", then a copy of any message that goes to "MyTopic" will go into the queue for both "S1" and "S2".
    * If there are two instances of a **Service** ("C1" and "C2") that ask for a message, the first client to ask will get the next message in the **Service** queue they are a member of.
