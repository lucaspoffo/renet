import * as wasm from 'renet_webtransport_example';

/**
 * @type {wasm.ChatApplication}
 */
let app;
let messages_to_send = [];
init();

/**
 *
 * @param {HTMLDivElement} output
 */
function update(output) {
  if (app) {
    app.update();
    let messages = app.get_messages();
    app.clear_messages();
    if (output) {
      for (let message of messages.split('\n')) {
        output.innerHTML += '<p>' + message + '</p>';
      }
    }
  }
}

function send_packets() {
  if (app) {
    app.send_packets();
  }
}

function init() {
  // get the chat elements
  const chat = document.getElementById('chat-input');
  const username = document.getElementById('name-input');
  const send = document.getElementById('chat-send');
  const output = document.getElementById('chat-output');

  // send message when button is clicked
  send.addEventListener('click', () => {
    if (app) {
      messages_to_send.push(username.value + ': ' + chat.value);
      chat.value = '';
    }
  });

  // send message when enter is pressed
  chat.addEventListener('keyup', (e) => {
    if (e.key === 'Enter') {
      send.click();
    }
  });

  wasm.ChatApplication.new()
    .then(async (_app) => {
      app = _app;
      console.log('App created!');
      while (true) {
        if (app.is_disconnected()) {
          console.log('Disconnected!');
          break;
        }
        send_packets();
        update(output);
        for (let message of messages_to_send) {
          app.send_message(message);
        }
        messages_to_send = [];
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      console.log('App disconnected!');
      console.log(app.is_disconnected());
    })
    .catch((err) => {
      console.error(err);
      console.log(app.is_disconnected());
    });
}

// example for using WebTransport in js
// const url = 'https://127.0.0.1:4433';
// const transport = new WebTransport(url);
// /**
//  * @type {WritableStreamDefaultWriter<Uint8Array>}
//  */
// let writer;
// let reader;

// // Optionally, set up functions to respond to
// // the connection closing:
// transport.closed
//   .then(() => {
//     console.log(`The HTTP/3 connection to ${url} closed gracefully.`);
//   })
//   .catch((error) => {
//     console.error(`The HTTP/3 connection to ${url} closed due to ${error}.`);
//   });

// transport.ready.then(async () => {
//   console.log(`The HTTP/3 connection to ${url} is ready.`);
//   reader = transport.datagrams.readable.getReader();
//   writer = transport.datagrams.writable.getWriter();
//   while (true) {
//     const { value, done } = await reader.read();
//     if (done) {
//       console.log('No more datagrams.');
//       break;
//     }
//     console.log(`Datagram received: ${value}`);
//   }
// });

// setInterval(() => {
//   if (writer) {
//     writer.write(new Uint8Array([1, 2, 3])).then(() => {
//       console.log('Datagram sent.');
//     });
//   }
// }, 1000);
