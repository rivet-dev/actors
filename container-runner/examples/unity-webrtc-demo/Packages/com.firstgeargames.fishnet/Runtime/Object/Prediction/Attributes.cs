using System;

namespace FishNet.Object.Prediction
{
    /// <summary>
    /// Replicated methods are to be called from clients and will run the same data and logic on the server.
    /// Only data used as method arguments will be serialized.
    /// </summary>
    [AttributeUsage(AttributeTargets.Method, Inherited = true, AllowMultiple = false)]
    public class ReplicateAttribute : Attribute { }

    /// <summary>
    /// Reconcile methods indicate how to reset your script or object after the server has replicated user data.
    /// </summary>
    [AttributeUsage(AttributeTargets.Method, Inherited = true, AllowMultiple = false)]
    public class ReconcileAttribute : Attribute { }
}