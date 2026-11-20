rm -rf package
mkdir package

echo "{\"name\":\"pkg1\"}" > package/package.json
tar -cvf archive.tar package/package.json

# Append another package.json with the same path but different content
rm package/package.json
echo "{\"name\":\"pkg2\"}" > package/package.json
tar -rvf archive.tar package/package.json

rm -rf package
