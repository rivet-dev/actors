using FishNet.Documenting;
using FishNet.Utility;
using System;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.GENERATED_ASSEMBLY_NAME)]

namespace FishNet.Serializing
{
    /// <summary>
    /// Used to read generic types.
    /// </summary>
    [APIExclude]
    public static class GenericDeltaReader<T>
    {
        public static Func<Reader, T, T> Read { get; internal set; }
        /// <summary>
        /// True if this type has a custom writer.
        /// </summary>
        internal static bool HasCustomSerializer;

        public static void SetRead(Func<Reader, T, T> value)
        {
            /* If a custom serializer has already been set then exit method
             * to not overwrite serializer. */
            if (HasCustomSerializer)
                return;

            bool isGenerated = value.Method.Name.StartsWith(UtilityConstants.GeneratedReaderPrefix);
            /* If generated then see if a regular custom writer exists. If so
             * then do not set a serializer to a generated one. */
            // TODO Make it so DefaultDeltaReader methods are picked up by codegen.
            if (isGenerated && GenericReader<T>.HasCustomSerializer)
                return;

            // Set has custom serializer if value being used is not a generated method.
            HasCustomSerializer = !isGenerated;
            Read = value;
        }
    }
}