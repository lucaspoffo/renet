import * as wasm from "renet_webtransport_example";

let app;
wasm.ChatApplication.new().then((_app) => {
  app = _app;
});
app.update();

// example for using WebTransport in js
// const url = 'https://127.0.0.1:4433';

// const transport = new WebTransport(url);

// // Optionally, set up functions to respond to
// // the connection closing:
// transport.closed.then(() => {
//   console.log(`The HTTP/3 connection to ${url} closed gracefully.`);
// }).catch((error) => {
//   console.error(`The HTTP/3 connection to ${url} closed due to ${error}.`);
// });

// transport.ready.then(() => {
//     console.log(`The HTTP/3 connection to ${url} is ready.`);
// });