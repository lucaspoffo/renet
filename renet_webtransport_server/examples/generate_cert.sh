set -e

# double slash in -subj is because of https://github.com/openssl/openssl/issues/8795, for other platforms besides Windows, use -subj '/CN=Test Certificate' 
# do not increase days, because it will cause the cert to be not accepted by the browser
openssl req -new -newkey rsa:4096 -x509 -sha256 -nodes -days 3 -out localhost.crt -keyout localhost.key -subj '//CN=Test Certificate' -addext "subjectAltName = DNS:localhost"
openssl x509 -in localhost.crt -outform der -out localhost.der
openssl rsa -outform der -in localhost.key -out localhost_key.der

SPKI=`openssl x509 -inform der -in localhost.der -pubkey -noout | openssl pkey -pubin -outform der | openssl dgst -sha256 -binary | openssl enc -base64`

echo "Got cert key $SPKI"

read rm 
#"Press enter to continue"