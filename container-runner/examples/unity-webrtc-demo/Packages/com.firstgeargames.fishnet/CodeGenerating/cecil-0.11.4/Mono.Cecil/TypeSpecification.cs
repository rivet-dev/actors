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
    public abstract class TypeSpecification : TypeReference
    {
        public TypeReference ElementType { get; }
        public override string Name
        {
            get { return ElementType.Name; }
            set { throw new InvalidOperationException(); }
        }
        public override string Namespace
        {
            get { return ElementType.Namespace; }
            set { throw new InvalidOperationException(); }
        }
        public override IMetadataScope Scope
        {
            get { return ElementType.Scope; }
            set { throw new InvalidOperationException(); }
        }
        public override ModuleDefinition Module
        {
            get { return ElementType.Module; }
        }
        public override string FullName
        {
            get { return ElementType.FullName; }
        }
        public override bool ContainsGenericParameter
        {
            get { return ElementType.ContainsGenericParameter; }
        }
        public override MetadataType MetadataType
        {
            get { return (MetadataType)etype; }
        }

        internal TypeSpecification(TypeReference type) : base(null, null)
        {
            ElementType = type;
            token = new(TokenType.TypeSpec);
        }

        public override TypeReference GetElementType()
        {
            return ElementType.GetElementType();
        }
    }
}