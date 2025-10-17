@echo off
setlocal enabledelayedexpansion

REM ===== Simple variable operations =====
set NAME=World
echo Hello %NAME%

REM ===== Subroutine calls =====
call :greet "Alice" 25
call :greet "Bob" 30
echo Back in main after subroutines

REM ===== Goto example with delayed expansion =====
set COUNTER=0

:loop_start
set /a COUNTER+=1
echo Loop iteration !COUNTER!
if !COUNTER! LSS 3 goto :loop_start

REM ===== Nested calls (call stack test) =====
call :level1
echo Finished all nested calls

REM ===== Conditional execution =====
set VALUE=10
if %VALUE% GTR 5 (
    echo Value is greater than 5
    set RESULT=BIG
) else (
    echo Value is small
    set RESULT=SMALL
)

REM ===== Delayed expansion example =====
set VAR=original
for %%i in (1 2 3) do (
    set VAR=changed_%%i
    echo Normal expansion: %VAR%
    echo Delayed expansion: !VAR!
)

REM ===== Error handling =====
call :might_fail
if errorlevel 1 echo Previous command failed!

REM ===== End main execution =====
echo Script complete
exit /b 0

REM ===== SUBROUTINES BELOW =====

:greet
echo Hello %~1, you are %~2 years old
set GREETED=%~1
exit /b 0

:level1
echo Entering level1
call :level2
echo Exiting level1
exit /b 0

:level2
echo Entering level2
call :level3
echo Exiting level2
exit /b 0

:level3
echo In level3 - deepest level
set DEPTH=3
exit /b 0

:might_fail
echo This subroutine returns an error
exit /b 1
