using System;

namespace CodeBoost.CodeAnalysis
{

    /// <summary>
    /// Include this source when creating signatures.
    /// </summary>
    [AttributeUsage(AttributeTargets.All, Inherited = false)]
    internal sealed class CreateSignatureAttribute : Attribute { }
    /// <summary>
    /// Ignore this source when creating signatures.
    /// </summary>
    [AttributeUsage(AttributeTargets.All, Inherited = false)]
    [CreateSignature]
    internal sealed class IgnoreSignatureAttribute : Attribute { }
    /// <summary>
    /// Preserve the logic of this source when creating signatures.
    /// </summary>

    [AttributeUsage(validOn: AttributeTargets.Method | AttributeTargets.Property, Inherited = false)]
    [CreateSignature]
    internal sealed class PreserveLogicAttribute : Attribute { }

    /// <summary>
    /// Indicates that a member must be reset when an object is pooled.
    /// </summary>
    /// <remarks>While this attribute is present code generation should ensure the value is reset within IPoolResettable.OnReturn or PoolResettableAttribute methods.</remarks>
    [AttributeUsage(AttributeTargets.Property | AttributeTargets.Field)]
    [CreateSignature]
    public sealed class PoolResettableMemberAttribute : Attribute { }

    /// <summary>
    /// Indicates that a method handles resetting or initializing one or more members attributed with PoolResettableMemberAttribute.
    /// </summary>
    /// <remarks>Attributed methods are expected to execute when appropriate to reset or initializing members.</remarks>
    [AttributeUsage(AttributeTargets.Method)]
    [CreateSignature]
    public sealed class PoolResettableMethodAttribute : Attribute { }

    /// <summary>
    /// Indicates that a member must be disposed when an object is pooled.
    /// </summary>
    /// <remarks>While this attribute is present code generation should ensure the value is disposed within IPoolResettable.OnReturn or PoolResettableAttribute methods.</remarks>
    [AttributeUsage(AttributeTargets.Property | AttributeTargets.Field)]
    [CreateSignature]
    public sealed class PoolDisposableMemberAttribute : Attribute { }
}
