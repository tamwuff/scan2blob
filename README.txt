Note that russh seems to have trouble with RSA keys. The instructions below
are for creating an ed25519 host key, and this is the recommended algorithm for
user keys as well.

To create an SSH key that will work with russh, run:

% ssh-keygen -f mykey -t ed25519

That will create "mykey" and "mykey.pub".

If you want to convert the private key into a format that will be paste-able
between double quotes to become a JSON string, try running:

% tr \\012 '~' < mykey | sed 's/~/\\n/g'

If you don't want to do that, you can also point directly at a filesystem path
and have scan2blob load the key from there, too.

(Public SSH keys don't have this problem because they're always a single line
of text, so they can be pasted between double quotes with no hassle)
