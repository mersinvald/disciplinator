#!/usr/bin/env bash

CMD=$1
SOURCE=$2
DESTINATION=$3

if [[ ! -d $SOURCE ]]
then
    echo $SOURCE not found
    exit 1
fi

if [[ "$CMD" == "save" ]]
then
    rm -rf $DESTINATION > /dev/null
    mkdir -p $DESTINATION
fi

SOURCE=$(realpath "$SOURCE")
DESTINATION=$(realpath "$DESTINATION")

PATTERNS=("-type d -name target" "-type f -name Cargo.lock")
IGNORE=$(basename $DESTINATION)

function run_recursive_swap() {
    for pattern in "${PATTERNS[@]}"
    do
        echo running caching pattern \"$pattern\"
        pushd $SOURCE > /dev/null
        find . $pattern -not -path "$IGNORE/*" -exec sh -c '
          echo moving $1 into "$0/${1%/*}"
          mkdir -p "$0/${1%/*}"
          mv "$1" "$0/$1"
        ' "$DESTINATION" {} \; 2> /dev/null
        popd > /dev/null
    done
}

if [[ "$CMD" == "save" ]]
then
    echo "saving cache $SOURCE -> $DESTINATION"
    run_recursive_swap
elif [[ "$CMD" == "load" ]]
then
    echo "loading cache $SOURCE -> $DESTINATION"
    run_recursive_swap
else
    echo "unknown command"
    exit 1
fi
