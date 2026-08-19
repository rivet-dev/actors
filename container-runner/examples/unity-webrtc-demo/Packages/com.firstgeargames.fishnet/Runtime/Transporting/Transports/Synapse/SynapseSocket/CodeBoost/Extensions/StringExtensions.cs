using System.Text;
using CodeBoost.Performance;

namespace CodeBoost.Extensions
{

    public static partial class StringExtensions
    {
        /// <summary>
        /// Value representing when an index is not found or specified.
        /// </summary>
        public const int UnsetIndex = -1;
            
        /// <summary>
        /// Converts a member string text to PascalCase
        /// </summary>
        /// <remarks>Leading non-alpha characters are removed and the first alpha character is capitalized.</remarks>
        public static string MemberToPascalCase(this string value)
        {
            int index = value.GetFirstLetterOrDigitIndex();

            //Index not found. String is null or has no chars/numbers.
            if (index == UnsetIndex)
                return value;

            char firstValidChar = value[index];

            //First character is not a letter, return as-is.
            if (!char.IsLetter(firstValidChar))
                return value;

            //Already capitalized.
            if (char.IsUpper(firstValidChar))
                return value;

            StringBuilder stringBuilder = ObjectPool<StringBuilder>.Rent();
            stringBuilder.Clear();

            stringBuilder.Append(char.ToUpperInvariant(firstValidChar));
            stringBuilder.Append(value.Substring(index + 1));

            string result = stringBuilder.ToString();
            ObjectPool<StringBuilder>.Return(stringBuilder);

            return result;
        }


        /// <summary>
        /// Converts a pascal case string to member case with an optional prefix.
        /// </summary>
        /// <example>With a prefix of '_' value 'HelloWorld' is returned as '_helloWorld'.</example>
        /// <remarks>Prefix is only added if missing.</remarks>
        public static string PascalCaseToMember(this string value, string prefix = "_")
        {
            int index = value.GetFirstLetterOrDigitIndex();

            //Index not found. String is null or has no chars/numbers.
            if (index == UnsetIndex)
                return value;

            char firstValidChar = value[index];
            int prefixLength = prefix.Length;
                
            /* There are marginally more efficient ways to handle these prefix operations
             * but allocations are going to occur either way - use what is easier to read. */
            StringBuilder stringBuilder = ObjectPool<StringBuilder>.Rent();
            stringBuilder.Clear();

            //There is a prefix.
            if (prefixLength > 0)
            {
                if (prefixLength >= value.Length)
                {
                    stringBuilder.Append(prefix);

                    AppendLowerFirstCharAndRemainingValue();

                    return stringBuilder.ToString();
                }

                //If prefix is not yet added then do so.
                if (value.Substring(0, prefixLength) != prefix)
                    stringBuilder.Append(prefix);
            }

            //Add renaming with lowercase char.
            AppendLowerFirstCharAndRemainingValue();
                
            string result = stringBuilder.ToString();
            ObjectPool<StringBuilder>.Return(stringBuilder);

            return result;

            //Appends lowercase first char, and any renaming text in value.
            void AppendLowerFirstCharAndRemainingValue()
            {
                stringBuilder.Append(char.ToLowerInvariant(firstValidChar));
                //If value has enough length remaining append it as well.
                if (value.Length >= index)
                    stringBuilder.Append(value.Substring(index + 1));
            }
        }

        /// <summary>
        /// Converts a string into a byte array.
        /// </summary>
        /// <returns>Number of bytes written to buffer.</returns>
        /// <remarks>Buffer is instantiated as a new array if it is not large enough.</remarks>
        public static byte[] ToBytesNonAllocated(this string value, out int bytesWritten)
        {
            int valueLength = value.Length;
            UTF8Encoding encoding = Utf8EncodingPool.Rent();

            // Number of minimum bytes the buffer must be.
            int bytesNeeded = encoding.GetMaxByteCount(valueLength);

            byte[] array = System.Buffers.ArrayPool<byte>.Shared.Rent(bytesNeeded);

            bytesWritten = encoding.GetBytes(value, charIndex: 0, valueLength, array, byteIndex: 0);

            Utf8EncodingPool.Return(encoding);

            return array;
        }
            
        /// <summary>
        /// Returns index of the first letter or number in a string.
        /// </summary>
        public static int GetFirstLetterOrDigitIndex(this string value)
        {
            if (string.IsNullOrWhiteSpace(value))
                return UnsetIndex;

            for (int i = 0; i < value.Length; i++)
            {
                if (char.IsLetterOrDigit(value[i]))
                    return i;
            }

            return UnsetIndex;
        }
    }
}
