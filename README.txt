Note that russh seems to have trouble with 4096 bit RSA keys. Ed25519 seems
to work ok with russh, but it's not supported by the Brother ADS-1800W
scanner. 2048 bit RSA seems to work ok with both russh and the scanner.

To create an SSH key:

% ssh-keygen -f mykey -t rsa -b 2048

That will create "mykey" and "mykey.pub".

If you want to convert the private key into a format that will be paste-able
between double quotes to become a JSON string, try running:

% tr \\012 '~' < mykey | sed 's/~/\\n/g'

If you don't want to do that, you can also point directly at a filesystem path
and have scan2blob load the key from there, too.

(Public SSH keys don't have this problem because they're always a single line
of text, so they can be pasted between double quotes with no hassle)

Appendix:

Here are some useful URLs:
https://datatracker.ietf.org/doc/html/rfc4254
https://datatracker.ietf.org/doc/html/rfc4252
https://datatracker.ietf.org/doc/html/draft-ietf-secsh-filexfer-02
https://datatracker.ietf.org/doc/html/rfc4918
https://github.com/paperless-ngx/paperless-ngx/wiki/Scanner-&-Software-Recommendations
