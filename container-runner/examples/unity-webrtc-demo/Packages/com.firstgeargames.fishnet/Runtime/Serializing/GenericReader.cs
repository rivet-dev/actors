using FishNet.Documenting;
using FishNet.Utility;
using System;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.GENERATED_ASSEMBLY_NAME)]
//Required for internal tests.
[assembly: InternalsVisibleTo(UtilityConstants.TEST_ASSEMBLY_NAME)]

namespace FishNet.Serializing
{
    /// <summary>
    /// Used to read generic types.
    /// </summary>
    [APIExclude]
    public static class GenericReader<T>
    {
        public static Func<Reader, T> Read { get; set; }
        /// <summary>
        /// True if this type has a custom writer.
        /// </summary>
        internal static bool HasCustomSerializer;

        public static void SetRead(Func<Reader, T> value)
        {
            /* If a custom serializer has already been set then exit method
             * to not overwrite serializer. */
            if (HasCustomSerializer)
                return;

            bool isGenerated = value.Method.Name.StartsWith(UtilityConstants.GeneratedReaderPrefix);

            //If not generated then unset any generated delta serializer.
            if (!isGenerated && GenericDeltaReader<T>.HasCustomSerializer)
                GenericDeltaReader<T>.Read = null;

            //Set has custom serializer if value being used is not a generated method.
            HasCustomSerializer = !isGenerated;
            Read = value;
        }
    }
}