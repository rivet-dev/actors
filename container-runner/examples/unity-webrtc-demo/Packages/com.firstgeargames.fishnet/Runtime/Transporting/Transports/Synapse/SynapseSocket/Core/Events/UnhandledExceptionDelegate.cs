using System;

namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.UnhandledException"/>.
    /// </summary>
    /// <param name="exception">The exception that escaped the background loop.</param>
    public delegate void UnhandledExceptionDelegate(Exception exception);
}
