using System;

namespace LiteNetLib.Utils
{
    /// <summary>
    ///     <para>PreserveAttribute prevents byte code stripping from removing a class, method, field, or property.</para>
    /// </summary>
    [AttributeUsage(AttributeTargets.Assembly | AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Enum | AttributeTargets.Constructor | AttributeTargets.Method | AttributeTargets.Property | AttributeTargets.Field | AttributeTargets.Event | AttributeTargets.Interface | AttributeTargets.Delegate, Inherited = false)]
    public class PreserveAttribute : Attribute { }
}