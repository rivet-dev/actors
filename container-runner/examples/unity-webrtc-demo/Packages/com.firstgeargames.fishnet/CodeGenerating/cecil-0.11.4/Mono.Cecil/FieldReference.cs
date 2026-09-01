//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;

namespace MonoFN.Cecil
{
    public class FieldReference : MemberReference
    {
        public TypeReference FieldType { get; set; }
        public override string FullName
        {
            get { return FieldType.FullName + " " + MemberFullName(); }
        }
        public override bool ContainsGenericParameter
        {
            get { return FieldType.ContainsGenericParameter || base.ContainsGenericParameter; }
        }

        internal FieldReference()
        {
            token = new(TokenType.MemberRef);
        }

        public FieldReference(string name, TypeReference fieldType) : base(name)
        {
            Mixin.CheckType(fieldType, Mixin.Argument.fieldType);

            FieldType = fieldType;
            token = new(TokenType.MemberRef);
        }

        public FieldReference(string name, TypeReference fieldType, TypeReference declaringType) : this(name, fieldType)
        {
            Mixin.CheckType(declaringType, Mixin.Argument.declaringType);

            DeclaringType = declaringType;
        }

        protected override IMemberDefinition ResolveDefinition()
        {
            return Resolve();
        }

        public new virtual FieldDefinition Resolve()
        {
            ModuleDefinition module = Module;
            if (module == null)
                throw new NotSupportedException();

            return module.Resolve(this);
        }
    }
}