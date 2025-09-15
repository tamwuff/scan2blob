Note that russh seems to have trouble with RSA keys. The instructions below
are for creating an ed25519 host key, and this is the recommended algorithm for
user keys as well.

To create an SSH host key, run:

> ssh-keygen -f mykey -t ed25519

That will create "mykey" and "mykey.pub". Delete "mykey.pub".

You can convert "mykey" into a format that will be paste-able between double
quotes to become a JSON string, by running:

> tr \\012 '~' < mykey | sed 's/~/\\n/g'
