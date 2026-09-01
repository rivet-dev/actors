using FishNet.Documenting;
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
    public static class GenericWriter<T>
    {
        public static Action<Writer, T> Write { get; private set; }
        /// <summary>
        /// True if this type has a custom writer.
        /// </summary>
        internal static bool HasCustomSerializer;

        public static void SetWrite(Action<Writer, T> value)
        {
            /* If a custom serializer has already been set then exit method
             * to not overwrite serializer. */
            if (HasCustomSerializer)
                return;

            bool isGenerated = value.Method.Name.StartsWith(UtilityConstants.GeneratedWriterPrefix);

            // If not generated then unset any generated delta serializer.
            if (!isGenerated && GenericDeltaWriter<T>.HasCustomSerializer)
                GenericDeltaWriter<T>.Write = null;

            // Set has custom serializer if value being used is not a generated method.
            HasCustomSerializer = !isGenerated;
            Write = value;
        }
    }
}