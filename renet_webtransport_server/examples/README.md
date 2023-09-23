To get a cerificate for the server, you can use the following file:
generate_cert.sh
which you need to execute on windows in a git bash shell.
The script will generate a certificate and a key file in the current directory and install the CA certificate in the local certificate store and also in supported browsers.

# Requirements
- https://github.com/FiloSottile/mkcert (for generating certificates)

# Certificate generation
- Install mkcert
- Run `generate_cert.sh`
- Install the generated CA certificate in your browser, if the browser is not support by the mkcert -install

Note: The certificate is only valid for localhost, 127.0.0.1 and [::1], if you want to use a different hostname, you need to change the 'generate_cert.sh' script. 