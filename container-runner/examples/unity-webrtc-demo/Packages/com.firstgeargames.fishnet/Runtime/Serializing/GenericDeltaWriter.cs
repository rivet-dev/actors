using FishNet.Documenting;
using FishNet.Serializing;
using FishNet.Utility;
using System;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.GENERATED_ASSEMBLY_NAME)]

namespace FishNet.Serializing
{
    /// <summary>
    /// Used to write generic types.
    /// </summary>
    [APIExclude]
    public static class GenericDeltaWriter<T>
    {
        public static Func<Writer, T, T, DeltaSerializerOption, bool> Write { get; internal set; }
        /// <summary>
        /// True if this type has a custom writer.
        /// </summary>
        internal static bool HasCustomSerializer;

        public static void SetWrite(Func<Writer, T, T, DeltaSerializerOption, bool> value)
        {
            /* If a custom serializer has already been set then exit method
             * to not overwrite serializer. */
            if (HasCustomSerializer)
                return;

            bool isGenerated = value.Method.Name.StartsWith(UtilityConstants.GeneratedWriterPrefix);
            /* If generated then see if a regular custom writer exists. If so
             * then do not set a serializer to a generated one. */
            // TODO Make it so DefaultDeltaWriter methods are picked up by codegen.
            if (isGenerated && GenericWriter<T>.HasCustomSerializer)
                return;

            // Set has custom serializer if value being used is not a generated method.
            HasCustomSerializer = !isGenerated;
            Write = value;
        }
    }
}