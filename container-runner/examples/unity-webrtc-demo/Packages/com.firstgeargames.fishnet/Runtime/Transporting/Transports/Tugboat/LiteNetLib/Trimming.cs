#if NET5_0_OR_GREATER
using System.Diagnostics.CodeAnalysis;
using static System.Diagnostics.CodeAnalysis.DynamicallyAccessedMemberTypes;

namespace LiteNetLib
{
    internal static class Trimming
    {
        internal const DynamicallyAccessedMemberTypes SerializerMemberTypes = PublicProperties | NonPublicProperties;
    }
}
#endif