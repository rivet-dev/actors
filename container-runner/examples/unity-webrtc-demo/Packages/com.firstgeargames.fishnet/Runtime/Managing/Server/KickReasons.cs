namespace FishNet.Managing.Server
{
    public enum KickReason : short
    {
        /// <summary>
        /// No reason was specified.
        /// </summary>
        Unset = 0,
        /// <summary>
        /// Client performed an action which could only be done if trying to exploit the server.
        /// </summary>
        ExploitAttempt = 1,
        /// <summary>
        /// Data received from the client could not be parsed. This rarely indicates an attack.
        /// </summary>
        MalformedData = 2,
        /// <summary>
        /// Client sent more data than should be able to.
        /// </summary>
        ExploitExcessiveData = 3,
        /// <summary>
        /// Client has sent a large amount of data several times in a row. This may not be an attack but there is no way to know with certainty.
        /// </summary>
        ExcessiveData = 4,
        /// <summary>
        /// A problem occurred with the server where the only option was to kick the client. This rarely indicates an exploit attempt.
        /// </summary>
        UnexpectedProblem = 5,
        /// <summary>
        /// Client is behaving unusually, such as providing multiple invalid states. This may not be an attack but there is no way to know with certainty.
        /// </summary>
        UnusualActivity = 6
    }
}