import * as wasm from 'renet_webtransport_example';

let app;
wasm.ChatApplication.new().then((_app) => {
  app = _app;
  console.log("App created!");
  if (app) {
    app.send_message("Hello from JS!");
    console.log("Message sent!");
  }
  app.send_packets().then(() => {
    console.log("Packets sent!");
    app.update().then(() => {
      console.log("App updated!");
      let messages = app.get_messages();
      console.log(messages);
    }).catch((err) => {
      console.error("App update error:", err);
    });
  });
});
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
