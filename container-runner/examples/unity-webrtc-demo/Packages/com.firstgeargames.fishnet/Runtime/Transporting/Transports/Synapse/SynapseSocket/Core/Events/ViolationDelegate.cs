namespace SynapseSocket.Core.Events
{

    /// <summary>
    /// Delegate for <see cref="SynapseManager.ViolationDetected"/>.
    /// <para>
    /// The returned <see cref="ViolationAction"/> tells the engine how to respond to the violation.
    /// Because C# multicast delegates return only the last subscriber's value, attaching more than one
    /// handler will silently discard all but the final return value. Keep at most one subscriber if
    /// the returned action matters; attach multiple subscribers only for side-effects (e.g. logging).
    /// </para>
    /// </summary>
    /// <param name="violationEventArgs">
    /// Details about the violation, including the reason, the offending endpoint, and the engine's
    /// default action.
    /// </param>
    /// <returns>
    /// The <see cref="ViolationAction"/> the engine should take. Return the value from
    /// <see cref="ViolationEventArgs.DefaultAction"/> to accept the engine default.
    /// </returns>
    public delegate ViolationAction ViolationDelegate(ViolationEventArgs violationEventArgs);
}
