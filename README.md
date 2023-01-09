# `on-the-gpu`

I use this program to quickly put something on the GPU.

In Linux, on a system with two GPUs, by default, stuff runs on a iGPU. To run
on the GPU, you need to wrap in either `pvkrun` (for Vulkan), or in `primusrun`
(for OpenGL)¹. This script makes it easy to do that.

It also logs the arguments it is being invoked with. This is helpful in some
circumstances to debug a program; usually, it ends up being something like:

* A game is not starting
* Steam doesn't log the `stdout`/`stderr` of the game
* I want to run it manually in a terminal to get those logs
* I need to know how to invoke it

Hence, the argument & CWD logging.

I'm merging a bunch of shell scripts into this, essentially. Some features that
my various shell wrappers have that I need to still incorporate:

* Logging to a file. (E.g., AFAIK, Steam doesn't do anything with `stderr` of
  games; this hinders the aforementioned argument logging, too.)
* A switch for doing `primusrun`, instead of `pvkrun`.

It's a pretty small wrapper at the moment. "Shell is easier" — that's nice. I
don't like shell.

¹I am not sure of this. `pvkrun` seems to work for some programs that appear to
be OpenGL based, too? Also, it's not *strictly* true that it's required: a
program not wrapped in `pvkrun`, if it queries the GPUs, will see both, and is
free to select the dGPU. (But that usually gets listed second, and most apps
appear to just select the first.) Additionally, apps seems to crash (on
shutdown?) if not wrapped `pvkrun` and if they select the dGPU.
