# OSC Command Dispatcher

This module allows us to convert OSC commands into shell commands on the Raspberry Pi. Designed for integration with QLab, Ion etc that has wider support than SSH-ing in and running commands. This is managed by a configuration and supports OSC parameters in command strings. 

## Configuration 

The configuration file is defined by a JSON Schema in `src/main.rs`. The file must be a JSON object with a `bind` key with a string value of the IP address to which the service should bind (for example `0.0.0.0`), `port` key with a number value of the port on which the server should bind (for example `6666`) and `commands` which maps OSC paths to shell commands in the following format

`
"/path": "echo $0"
`

Where `/path` is the OSC command address, and `echo $1` is the command. This also shows the argument substitution that is performed. Any value `$n` where `n` is a number that is in range of the number of arguments will be replaced. Note: there is no argument count requirement currently implemented so if you have a command with 3 arguments (`echo $0 $1 $2`) and you send a command with a single value (`1`) you will get `echo 1 $1 $2`. This may be addressed in the future but is not currently. Any OSC string parameters should be escaped properly. 

### Formatting

OSC types are fomatted as so

|Type|Format|
|--|--|
|Int (0)|`0`|
|Float (0.0) (1.3)|`0` `1.3`|
|String (abc)|`"abc"`|
|Time (12 seconds 43 fractional)|`12.43`|
|Long (0)|`0`|
|Double (0.0)|`0.0`|
|Char (a)|`a`|
|Color (R=255, G=128, B=2, A=14)|`rgba(255, 128, 2, 14)`|
|Bool (true)|`true`|
|Nil|`null`|
|Inf|`inf`|

## Service

A systemd service file is included for registering on the raspberry pi. This has not yet been tested