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

#### API Support
 
Currently only Fitbit API is supported as an author is a proud owner of the Charge 2. Support of the other APIs is not planned
until author decides to switch to another fitness wearables manufacturer.

#### Acknowledgements

Huge thanks to [@bradfordboyle](https://github.com/bradfordboyle) for his [fitbit-grabber-rs](https://github.com/bradfordboyle/fitbit-grabber-rs) crate, 
which I'm [continuing to develop](https://github.com/mersinvald/fitbit-grabber-rs) for the needs of this project.

#### Contacts

Mike Lubinets: public@mersinvald.me