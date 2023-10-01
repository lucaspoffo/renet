import * as wasm from 'renet_webtransport_example';

/**
 * @type {wasm.ChatApplication}
 */
let app;

init();

/**
 * 
 * @param {HTMLDivElement} output 
 */
async function update(output) {
  if (app) {
    app.update();
    let messages = app.get_messages();
    app.clear_messages();
    // console.log(messages);
    if (output) {
      for (let message of messages.split('\n')) {
        output.innerHTML += '<p>' + message + '</p>';
      }
    }
  }
}

async function send_packets() {
  if (app) {
    await app.send_packets();
    // console.log('Packets sent!');
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
      app.send_message(username.value + ': ' + chat.value);
      chat.value = '';
    }
  });

  // send message when enter is pressed
  chat.addEventListener('keyup', (e) => {
    if (e.key === 'Enter') {
      send.click();
    }
  });

  wasm.ChatApplication.new().then(async (_app) => {
    app = _app;
    console.log('App created!');
    while (true) {
      await send_packets();
      await update(output);
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  });
}


















// // example for using WebTransport in js
// const url = 'https://127.0.0.1:4433';
// const transport = new WebTransport(url);

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
//   while (true) {
//     const { value, done } = await transport.datagrams.readable
//       .getReader()
//       .read();
//     if (done) {
//       console.log('No more datagrams.');
//       break;
//     }
//     console.log(`Datagram received: ${value}`);
//   }
// });

// setInterval(() => {
//   if (!transport.datagrams.writable.locked) {
//     transport.datagrams.writable
//       .getWriter()
//       .write(new Uint8Array([1, 2, 3]))
//       .then(() => {
//         console.log('Datagram sent.');
//       });
//   }
// }, 1000);
