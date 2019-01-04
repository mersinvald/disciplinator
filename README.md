[![Build Status](https://travis-ci.com/mersinvald/disciplinator.svg?branch=master)](https://travis-ci.com/mersinvald/disciplinator)

### Disciplinator [WORK IN PROGRESS]
Disciplinator is a motivatory tool that utilizes Fitbit personal API. It is designed to help in breaking the sedentary 
lifestyle through motivating the user to keep being active each hour (except for sleeping hours and some evening Pizza Time).

#### How it works?

`Disciplinator` has 3 main parts:

- `Headmaster`: web-service that connects to the Fitbit API, calculates the __`activity debt`__ and returns current status upon request:
  either Normal or DebtCollection(N), where N is number of minutes to work out to change state back to normal.
- `Priestess` is an intermediate helper library to provide common Data Types among different Fitness Apps (currently only Fitbit API is supported).
- `Driver` is a client-side part that would motivate user through making his life harder if they fail to keep active through the day.
  It uses data provided by the `headmaster` to perform the callback-defined actions.

Principles and unresolved questions are described in the [Rule Book](rulebook.md).

##### Drivers

As the `driver` library is in principle indiscriminate in what one would do with the received events, and can trigger any
external callbacks as long as they fall into the `Box<Fn(headmaster::State) -> Result<(), failure::Error>` interface, it's possible to
define any actions to be triggered as events.

This repository ships the `executor` binary in `driver/drivers/executor` which is a handy mediator between the `driver` lib and user-defined actions.
Actions are represented as `Plugins`, which are just executable files with a manifest file attached to them. Plugins may be 
implemented in any language, be it `bash`, `python`, `ruby`, or even `PowerShell` is one feels like it. Plugin may as well be 
a binary file produced by `Rust`, `C` or any other compiler. Anything as long as it's executable.

The plugin Manifest defines on which events plugin should be ran, and if it's enabled.

For a plugin sample please refer to the [osx_send_notification.sh](driver/drivers/executor/plugins/osx_send_notification.sh) bash script, 
and it's manifest file [osx_send_notification.sh.toml](driver/drivers/executor/plugins/osx_send_notification.sh.toml);


#### API Support
 
Currently only Fitbit API is supported as an author is a proud owner of the Charge 2. Support of the other APIs is not planned
until author decides to switch to another fitness wearables manufacturer.

#### Acknowledgements

Huge thanks to [@bradfordboyle](https://github.com/bradfordboyle) for his [fitbit-grabber-rs](https://github.com/bradfordboyle/fitbit-grabber-rs) crate, 
which I'm [continuing to develop](https://github.com/mersinvald/fitbit-grabber-rs) for the needs of this project.

#### Contacts

Mike Lubinets: public@mersinvald.me