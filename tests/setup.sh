#!/bin/bash

make start-server-detached
sleep 20

make run-tests
if [ $? -eq 0 ]
then
  echo "Tests passed successfully!"
  exit_status=0
else
  echo "Tests failed!" >&2
  exit_status=1
fi

docker stop secretdev

exit $exit_status