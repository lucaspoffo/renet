To get a cerificate for the server, you can use the following file:
generate_cert.sh
which you need to execute on windows in a git bash shell.
The script will generate a certificate and a key file in the current directory.

To let the browser accept the certificate, you need to activate this flags in chrome:
chrome://flags/#allow-insecure-localhost
chrome://flags/#unsafely-treat-insecure-origin-as-secure
chrome://flags/#enable-quic